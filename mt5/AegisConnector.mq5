//+------------------------------------------------------------------+
//| AegisConnector.mq5 — AEGIS connector for MetaTrader 5.            |
//|                                                                  |
//| Connects to the AEGIS engine, streams market/account data and     |
//| executes ONLY authenticated, unexpired, non-duplicated            |
//| instructions. It never decides to trade by itself.                |
//|                                                                  |
//| STATUS: NOT COMPILED / NOT VERIFIED on the development machine    |
//| (no MetaEditor). Compile in MetaEditor, run AegisSelfTest, then   |
//| validate on a DEMO account before anything else.                  |
//+------------------------------------------------------------------+
#property copyright "AEGIS"
#property version   "1.20"
#property strict
#property description "AEGIS connector: authenticated bridge to the AEGIS risk engine."

#include "AegisProtocol.mqh"

input string InpHost             = "127.0.0.1";   // Engine address (add it in Tools > Options > Expert Advisors)
input int    InpPort             = 7878;          // Engine port
input string InpSharedKey        = "";            // Shared key (shown in AEGIS > Connexion MT5)
input string InpSymbols          = "";            // Symbols, comma separated (empty = chart symbol)
input long   InpMagic            = 20260115;      // Magic number (must match the engine)
input bool   InpAllowRealAccount = false;         // Allow NEW entries on a REAL account
input int    InpTimerMs          = 100;           // Polling period (ms)

#define AEGIS_RETCODE_EXPIRED    90001
#define AEGIS_RETCODE_NOT_FOUND  90002
#define AEGIS_RETCODE_REFUSED    90003

//--- connection state
int    g_socket = INVALID_HANDLE;
bool   g_session = false;
uchar  g_psk[];
uchar  g_key[];
string g_sid = "";
long   g_seq_out = 0;
long   g_seq_in = 0;
uchar  g_rx[];
ulong  g_last_connect_ms = 0;
ulong  g_last_rx_ms = 0;
ulong  g_last_heartbeat_ms = 0;
ulong  g_last_account_ms = 0;
ulong  g_last_snapshot_ms = 0;
bool   g_snapshot_dirty = true;
long   g_snap_id = 0;
long   g_server_offset_ms = 0;

//--- market state
string g_symbols[];
long   g_last_tick_msc[];

//--- idempotency: instructions already handled in this run
string g_done_cid[];
string g_done_frame[];

//+------------------------------------------------------------------+
long NowMs() { return (long)TimeGMT() * 1000; }

// Broker server time -> UTC, rounded to a quarter of an hour.
void UpdateServerOffset()
  {
   long diff = (long)TimeTradeServer() - (long)TimeGMT();
   g_server_offset_ms = (long)MathRound((double)diff / 900.0) * 900 * 1000;
  }

//--- protocol 1.2: how the account holds positions (hedging accounts allow separate hedges)
string MarginModeName()
  {
   long m = AccountInfoInteger(ACCOUNT_MARGIN_MODE);
   if(m == ACCOUNT_MARGIN_MODE_RETAIL_HEDGING) return "hedging";
   if(m == ACCOUNT_MARGIN_MODE_RETAIL_NETTING) return "netting";
   if(m == ACCOUNT_MARGIN_MODE_EXCHANGE)       return "exchange";
   return "unknown";
  }

string TradeModeName()
  {
   long m = AccountInfoInteger(ACCOUNT_TRADE_MODE);
   if(m == ACCOUNT_TRADE_MODE_DEMO)    return "demo";
   if(m == ACCOUNT_TRADE_MODE_CONTEST) return "contest";
   return "real";
  }

//+------------------------------------------------------------------+
int OnInit()
  {
   if(StringLen(InpSharedKey) < 16)
     {
      Alert("AEGIS: the shared key is missing or too short (16 characters minimum).");
      return INIT_PARAMETERS_INCORRECT;
     }
   AegisBytes(InpSharedKey, g_psk);

   string list = (StringLen(InpSymbols) > 0 ? InpSymbols : _Symbol);
   int n = StringSplit(list, ',', g_symbols);
   ArrayResize(g_last_tick_msc, n);
   for(int i = 0; i < n; i++)
     {
      StringTrimLeft(g_symbols[i]);
      StringTrimRight(g_symbols[i]);
      if(!SymbolSelect(g_symbols[i], true))
        {
         Alert("AEGIS: unknown symbol ", g_symbols[i]);
         return INIT_PARAMETERS_INCORRECT;
        }
      g_last_tick_msc[i] = 0;
     }
   UpdateServerOffset();
   EventSetMillisecondTimer(MathMax(50, InpTimerMs));
   Print("AEGIS connector started; engine ", InpHost, ":", InpPort, "; account type: ", TradeModeName());
   return INIT_SUCCEEDED;
  }

void OnDeinit(const int reason)
  {
   EventKillTimer();
   if(g_session)
      SendBody(JStr("t", "bye") + "," + JStr("reason", "connector stopped"));
   Disconnect("connector stopped");
  }

//+------------------------------------------------------------------+
//| Transport                                                        |
//+------------------------------------------------------------------+
void Disconnect(const string why)
  {
   if(g_socket != INVALID_HANDLE)
     {
      SocketClose(g_socket);
      Print("AEGIS: disconnected (", why, ")");
     }
   g_socket = INVALID_HANDLE;
   g_session = false;
   g_sid = "";
   g_seq_in = 0;
   g_seq_out = 0;
   ArrayResize(g_rx, 0);
  }

bool SendRaw(const string frame)
  {
   uchar bytes[];
   AegisBytes(frame, bytes);
   int len = ArraySize(bytes);
   if(len <= 0 || len > AEGIS_MAX_FRAME)
      return false;
   int sent = SocketSend(g_socket, bytes, len);
   if(sent != len)
     {
      Disconnect("send failed");
      return false;
     }
   return true;
  }

// Send one session message (fields of the body, starting with "t").
bool SendBody(const string body_fields)
  {
   if(!g_session)
      return false;
   g_seq_out++;
   string id = StringSubstr(g_sid, 0, 8) + "-c" + IntegerToString(g_seq_out);
   return SendRaw(AegisSealFrame(g_key, id, NowMs(), g_seq_out, g_sid, body_fields));
  }

// Blocking read of one line, used only during the handshake.
bool ReadLineBlocking(string &line, const uint timeout_ms)
  {
   uchar buf[];
   uchar one[];
   ulong start = GetTickCount64();
   while(GetTickCount64() - start < timeout_ms)
     {
      uint avail = SocketIsReadable(g_socket);
      if(avail == 0)
        {
         Sleep(10);
         continue;
        }
      int got = SocketRead(g_socket, one, 1, 100);
      if(got != 1)
         continue;
      if(one[0] == '\n')
        {
         line = CharArrayToString(buf, 0, WHOLE_ARRAY, CP_UTF8);
         return true;
        }
      int n = ArraySize(buf);
      if(n >= AEGIS_MAX_FRAME)
         return false;
      ArrayResize(buf, n + 1);
      buf[n] = one[0];
     }
   return false;
  }

bool Connect()
  {
   g_socket = SocketCreate();
   if(g_socket == INVALID_HANDLE)
     {
      Print("AEGIS: SocketCreate failed, error ", GetLastError());
      return false;
     }
   if(!SocketConnect(g_socket, InpHost, InpPort, 3000))
     {
      int err = GetLastError();
      SocketClose(g_socket);
      g_socket = INVALID_HANDLE;
      if(err == 4014)
         Print("AEGIS: add ", InpHost, " to the allowed addresses (Tools > Options > Expert Advisors).");
      return false;
     }

   //--- hello (authenticated with the shared key)
   string nonce = AegisNonce();
   string hello = JStr("t", "hello") + "," + JStr("nonce", nonce) + "," + JStr("terminal", "mt5") + "," +
                  JInt("login", AccountInfoInteger(ACCOUNT_LOGIN)) + "," + JStr("server", AccountInfoString(ACCOUNT_SERVER)) + "," +
                  JStr("trade_mode", TradeModeName()) + "," +
                  JStr("build", "AegisConnector 1.20 / MT5 build " + IntegerToString(TerminalInfoInteger(TERMINAL_BUILD)));
   if(!SendRaw(AegisSealFrame(g_psk, "hello-" + nonce, NowMs(), 0, "", hello)))
      return false;

   //--- welcome
   string line;
   if(!ReadLineBlocking(line, 5000))
     {
      Disconnect("no welcome (wrong key, or PC clock more than 30 s off?)");
      return false;
     }
   string json = AegisOpenFrame(g_psk, line);
   string type, server_nonce, sid;
   if(json == "" || !AegisJsonString(json, "t", type) || type != "welcome" ||
      !AegisJsonString(json, "nonce", server_nonce) || !AegisJsonString(json, "sid", sid) || sid == "")
     {
      Disconnect("welcome rejected");
      return false;
     }
   if(!AegisSessionKey(g_psk, nonce, server_nonce, g_key))
     {
      Disconnect("key derivation failed");
      return false;
     }
   g_sid = sid;
   g_seq_in = 0;
   g_seq_out = 0;
   g_session = true;
   g_last_rx_ms = GetTickCount64();
   g_snapshot_dirty = true;
   Print("AEGIS: session established");

   //--- initial state
   UpdateServerOffset();
   for(int i = 0; i < ArraySize(g_symbols); i++)
      SendSpec(g_symbols[i]);
   SendAccount();
   SendSnapshot();
   return true;
  }

//+------------------------------------------------------------------+
//| Outgoing data                                                    |
//+------------------------------------------------------------------+
void SendSpec(const string symbol)
  {
   double point = SymbolInfoDouble(symbol, SYMBOL_POINT);
   double tick_size = SymbolInfoDouble(symbol, SYMBOL_TRADE_TICK_SIZE);
   double tick_value = SymbolInfoDouble(symbol, SYMBOL_TRADE_TICK_VALUE);
   //--- swaps converted to account currency per lot per night when the mode allows it
   long swap_mode = SymbolInfoInteger(symbol, SYMBOL_SWAP_MODE);
   double swap_long = SymbolInfoDouble(symbol, SYMBOL_SWAP_LONG);
   double swap_short = SymbolInfoDouble(symbol, SYMBOL_SWAP_SHORT);
   if(swap_mode == SYMBOL_SWAP_MODE_POINTS && tick_size > 0)
     {
      swap_long = swap_long * point / tick_size * tick_value;
      swap_short = swap_short * point / tick_size * tick_value;
     }
   else if(swap_mode != SYMBOL_SWAP_MODE_CURRENCY_DEPOSIT)
     {
      // Other modes cannot be converted reliably here: report zero (documented limitation).
      swap_long = 0.0;
      swap_short = 0.0;
     }
   long mode = SymbolInfoInteger(symbol, SYMBOL_TRADE_MODE);
   string trade_mode = (mode == SYMBOL_TRADE_MODE_FULL ? "full" : (mode == SYMBOL_TRADE_MODE_CLOSEONLY ? "closeonly" : "disabled"));
   SendBody(JStr("t", "spec") + "," + JStr("symbol", symbol) + "," + JInt("digits", SymbolInfoInteger(symbol, SYMBOL_DIGITS)) + "," +
            JNum("point", point, 10) + "," + JNum("contract_size", SymbolInfoDouble(symbol, SYMBOL_TRADE_CONTRACT_SIZE)) + "," +
            JNum("tick_size", tick_size, 10) + "," + JNum("tick_value", tick_value, 10) + "," +
            JNum("volume_min", SymbolInfoDouble(symbol, SYMBOL_VOLUME_MIN)) + "," + JNum("volume_max", SymbolInfoDouble(symbol, SYMBOL_VOLUME_MAX)) + "," +
            JNum("volume_step", SymbolInfoDouble(symbol, SYMBOL_VOLUME_STEP)) + "," +
            JNum("stops_level", (double)SymbolInfoInteger(symbol, SYMBOL_TRADE_STOPS_LEVEL), 1) + "," +
            JNum("swap_long", swap_long) + "," + JNum("swap_short", swap_short) + "," + JStr("trade_mode", trade_mode) + "," +
            JStr("currency_profit", SymbolInfoString(symbol, SYMBOL_CURRENCY_PROFIT)) + "," +
            //--- protocol 1.1: base currency (exposure per currency) and freeze level (SL/TP changes)
            JStr("currency_base", SymbolInfoString(symbol, SYMBOL_CURRENCY_BASE)) + "," +
            JNum("freeze_level", (double)SymbolInfoInteger(symbol, SYMBOL_TRADE_FREEZE_LEVEL), 1));
  }

void SendAccount()
  {
   bool allowed = AccountInfoInteger(ACCOUNT_TRADE_ALLOWED) && TerminalInfoInteger(TERMINAL_TRADE_ALLOWED) && MQLInfoInteger(MQL_TRADE_ALLOWED);
   SendBody(JStr("t", "account") + "," + JInt("login", AccountInfoInteger(ACCOUNT_LOGIN)) + "," + JStr("server", AccountInfoString(ACCOUNT_SERVER)) + "," +
            JStr("currency", AccountInfoString(ACCOUNT_CURRENCY)) + "," + JNum("balance", AccountInfoDouble(ACCOUNT_BALANCE), 2) + "," +
            JNum("equity", AccountInfoDouble(ACCOUNT_EQUITY), 2) + "," + JNum("margin", AccountInfoDouble(ACCOUNT_MARGIN), 2) + "," +
            JNum("margin_free", AccountInfoDouble(ACCOUNT_MARGIN_FREE), 2) + "," + JInt("leverage", AccountInfoInteger(ACCOUNT_LEVERAGE)) + "," +
            JStr("trade_mode", TradeModeName()) + "," + JBool("trade_allowed", allowed) + "," +
            JStr("margin_mode", MarginModeName()));
   g_last_account_ms = GetTickCount64();
  }

// Full picture of positions and pending orders, closed by a count check.
void SendSnapshot()
  {
   g_snap_id++;
   int n_pos = 0, n_ord = 0;
   for(int i = 0; i < PositionsTotal(); i++)
     {
      ulong ticket = PositionGetTicket(i);
      if(ticket == 0)
         continue;
      string side = (PositionGetInteger(POSITION_TYPE) == POSITION_TYPE_BUY ? "buy" : "sell");
      if(!SendBody(JStr("t", "pos") + "," + JInt("snap", g_snap_id) + "," + JInt("ticket", (long)ticket) + "," +
                   JStr("symbol", PositionGetString(POSITION_SYMBOL)) + "," + JStr("side", side) + "," +
                   JNum("volume", PositionGetDouble(POSITION_VOLUME)) + "," + JNum("price_open", PositionGetDouble(POSITION_PRICE_OPEN)) + "," +
                   JNum("sl", PositionGetDouble(POSITION_SL)) + "," + JNum("tp", PositionGetDouble(POSITION_TP)) + "," +
                   JNum("profit", PositionGetDouble(POSITION_PROFIT), 2) + "," + JNum("swap", PositionGetDouble(POSITION_SWAP), 2) + "," +
                   JNum("commission", 0.0, 2) + "," + JInt("magic", PositionGetInteger(POSITION_MAGIC)) + "," +
                   JStr("comment", PositionGetString(POSITION_COMMENT)) + "," +
                   JInt("open_time", PositionGetInteger(POSITION_TIME_MSC) - g_server_offset_ms)))
         return;
      n_pos++;
     }
   for(int i = 0; i < OrdersTotal(); i++)
     {
      ulong ticket = OrderGetTicket(i);
      if(ticket == 0)
         continue;
      long type = OrderGetInteger(ORDER_TYPE);
      string side, kind;
      if(type == ORDER_TYPE_BUY_LIMIT)       { side = "buy";  kind = "limit"; }
      else if(type == ORDER_TYPE_SELL_LIMIT) { side = "sell"; kind = "limit"; }
      else if(type == ORDER_TYPE_BUY_STOP)   { side = "buy";  kind = "stop"; }
      else if(type == ORDER_TYPE_SELL_STOP)  { side = "sell"; kind = "stop"; }
      else continue; // stop-limit orders are not managed by AEGIS
      if(!SendBody(JStr("t", "ord") + "," + JInt("snap", g_snap_id) + "," + JInt("ticket", (long)ticket) + "," +
                   JStr("symbol", OrderGetString(ORDER_SYMBOL)) + "," + JStr("side", side) + "," + JStr("kind", kind) + "," +
                   JNum("volume", OrderGetDouble(ORDER_VOLUME_CURRENT)) + "," + JNum("price", OrderGetDouble(ORDER_PRICE_OPEN)) + "," +
                   JNum("sl", OrderGetDouble(ORDER_SL)) + "," + JNum("tp", OrderGetDouble(ORDER_TP)) + "," +
                   JInt("magic", OrderGetInteger(ORDER_MAGIC)) + "," + JStr("comment", OrderGetString(ORDER_COMMENT)) + "," +
                   JInt("setup_time", OrderGetInteger(ORDER_TIME_SETUP_MSC) - g_server_offset_ms)))
         return;
      n_ord++;
     }
   SendBody(JStr("t", "snap_end") + "," + JInt("snap", g_snap_id) + "," + JInt("positions", n_pos) + "," + JInt("orders", n_ord));
   g_last_snapshot_ms = GetTickCount64();
   g_snapshot_dirty = false;
  }

void SendTicks()
  {
   MqlTick tick;
   for(int i = 0; i < ArraySize(g_symbols); i++)
     {
      if(!SymbolInfoTick(g_symbols[i], tick))
         continue;
      if(tick.time_msc == g_last_tick_msc[i] || tick.bid <= 0 || tick.ask < tick.bid)
         continue;
      g_last_tick_msc[i] = tick.time_msc;
      int digits = (int)SymbolInfoInteger(g_symbols[i], SYMBOL_DIGITS);
      SendBody(JStr("t", "tick") + "," + JStr("symbol", g_symbols[i]) + "," + JInt("time", tick.time_msc - g_server_offset_ms) + "," +
               JNum("bid", tick.bid, digits) + "," + JNum("ask", tick.ask, digits));
     }
  }

ENUM_TIMEFRAMES ParseTimeframe(const string tf)
  {
   if(tf == "M1")  return PERIOD_M1;
   if(tf == "M5")  return PERIOD_M5;
   if(tf == "M15") return PERIOD_M15;
   if(tf == "M30") return PERIOD_M30;
   if(tf == "H1")  return PERIOD_H1;
   return PERIOD_CURRENT;
  }

// Completed bars only (the bar in progress is skipped), oldest first, in UTC.
void SendHistory(const string symbol, const string tf, const int count)
  {
   ENUM_TIMEFRAMES period = ParseTimeframe(tf);
   if(period == PERIOD_CURRENT)
      return; // H4/D1 are not served: broker bars are not UTC-aligned
   MqlRates rates[];
   int got = CopyRates(symbol, period, 1, count, rates);
   int digits = (int)SymbolInfoInteger(symbol, SYMBOL_DIGITS);
   for(int i = 0; i < got; i++)
     {
      if(!SendBody(JStr("t", "bar") + "," + JStr("symbol", symbol) + "," + JStr("tf", tf) + "," +
                   JInt("time", (long)rates[i].time * 1000 - g_server_offset_ms) + "," +
                   JNum("open", rates[i].open, digits) + "," + JNum("high", rates[i].high, digits) + "," +
                   JNum("low", rates[i].low, digits) + "," + JNum("close", rates[i].close, digits) + "," +
                   JInt("volume", rates[i].tick_volume) + "," + JNum("spread", (double)rates[i].spread, 1) + "," +
                   JBool("last", i == got - 1)))
         return;
     }
  }

//+------------------------------------------------------------------+
//| Instructions                                                     |
//+------------------------------------------------------------------+
void SendResult(const string cid, const bool ok, const long retcode, const long ticket, const double price, const double volume, const string msg, const long broker_us = 0)
  {
   //--- protocol 1.2: broker_us = time spent inside OrderSend (0 when the broker was not called)
   string body = JStr("t", "result") + "," + JStr("cid", cid) + "," + JBool("ok", ok) + "," + JInt("retcode", retcode) + "," +
                 JInt("ticket", ticket) + "," + JNum("price", price) + "," + JNum("volume", volume) + "," + JStr("msg", StringSubstr(msg, 0, 200)) + "," +
                 JInt("broker_us", broker_us);
   //--- remember the answer so a duplicated instruction is answered, not executed again
   int n = ArraySize(g_done_cid);
   if(n >= 500)
     {
      ArrayRemove(g_done_cid, 0, 100);
      ArrayRemove(g_done_frame, 0, 100);
      n = ArraySize(g_done_cid);
     }
   ArrayResize(g_done_cid, n + 1);
   ArrayResize(g_done_frame, n + 1);
   g_done_cid[n] = cid;
   g_done_frame[n] = body;
   SendBody(body);
   g_snapshot_dirty = true;
  }

ENUM_ORDER_TYPE_FILLING FillingFor(const string symbol)
  {
   long modes = SymbolInfoInteger(symbol, SYMBOL_FILLING_MODE);
   if((modes & SYMBOL_FILLING_FOK) != 0) return ORDER_FILLING_FOK;
   if((modes & SYMBOL_FILLING_IOC) != 0) return ORDER_FILLING_IOC;
   return ORDER_FILLING_RETURN;
  }

// Ticket of a position / pending order already carrying this client id (after a restart).
long FindByComment(const string cid)
  {
   for(int i = 0; i < PositionsTotal(); i++)
     {
      ulong t = PositionGetTicket(i);
      if(t != 0 && PositionGetString(POSITION_COMMENT) == cid && PositionGetInteger(POSITION_MAGIC) == InpMagic)
         return (long)t;
     }
   for(int i = 0; i < OrdersTotal(); i++)
     {
      ulong t = OrderGetTicket(i);
      if(t != 0 && OrderGetString(ORDER_COMMENT) == cid && OrderGetInteger(ORDER_MAGIC) == InpMagic)
         return (long)t;
     }
   return 0;
  }

void HandleOrder(const string json)
  {
   string cid, action, symbol, side, kind;
   double volume = 0, price = 0, sl = 0, tp = 0, deviation = 0;
   long ticket = 0, expires = 0, magic = 0;
   if(!AegisJsonString(json, "cid", cid) || cid == "")
      return;
   bool complete = AegisJsonString(json, "action", action) && AegisJsonString(json, "symbol", symbol) &&
                   AegisJsonString(json, "side", side) && AegisJsonString(json, "kind", kind) &&
                   AegisJsonNumber(json, "volume", volume) && AegisJsonNumber(json, "price", price) &&
                   AegisJsonNumber(json, "sl", sl) && AegisJsonNumber(json, "tp", tp) &&
                   AegisJsonNumber(json, "deviation", deviation) && AegisJsonLong(json, "ticket", ticket) &&
                   AegisJsonLong(json, "expires", expires) && AegisJsonLong(json, "magic", magic);
   if(!complete)
     {
      SendResult(cid, false, AEGIS_RETCODE_REFUSED, 0, 0, 0, "malformed instruction");
      return;
     }

   //--- 1. duplicate: answer again, never execute twice
   for(int i = ArraySize(g_done_cid) - 1; i >= 0; i--)
      if(g_done_cid[i] == cid)
        {
         SendBody(g_done_frame[i]);
         return;
        }
   //--- 2. expiry (engine clock, UTC ms)
   if(NowMs() > expires)
     {
      SendResult(cid, false, AEGIS_RETCODE_EXPIRED, 0, 0, 0, "instruction expired");
      return;
     }
   if(magic != InpMagic)
     {
      SendResult(cid, false, AEGIS_RETCODE_REFUSED, 0, 0, 0, "magic number mismatch");
      return;
     }

   MqlTradeRequest req;
   MqlTradeResult res;
   ZeroMemory(req);
   ZeroMemory(res);
   req.magic = (ulong)InpMagic;
   req.symbol = symbol;
   req.comment = cid;
   double point = SymbolInfoDouble(symbol, SYMBOL_POINT);
   int digits = (int)SymbolInfoInteger(symbol, SYMBOL_DIGITS);

   if(action == "open")
     {
      if(AccountInfoInteger(ACCOUNT_TRADE_MODE) == ACCOUNT_TRADE_MODE_REAL && !InpAllowRealAccount)
        {
         SendResult(cid, false, AEGIS_RETCODE_REFUSED, 0, 0, 0, "real account: entries are disabled in the connector settings");
         return;
        }
      //--- already executed before a restart?
      long existing = FindByComment(cid);
      if(existing != 0)
        {
         SendResult(cid, false, AEGIS_RETCODE_REFUSED, existing, 0, 0, "already present on the account");
         return;
        }
      if(sl <= 0 || tp <= 0)
        {
         SendResult(cid, false, AEGIS_RETCODE_REFUSED, 0, 0, 0, "an entry must carry SL and TP");
         return;
        }
      //--- volume must be a legal volume for this symbol (never adjusted here)
      double vmin = SymbolInfoDouble(symbol, SYMBOL_VOLUME_MIN), vmax = SymbolInfoDouble(symbol, SYMBOL_VOLUME_MAX), vstep = SymbolInfoDouble(symbol, SYMBOL_VOLUME_STEP);
      double steps = (vstep > 0 ? volume / vstep : 0);
      if(volume < vmin - 1e-9 || volume > vmax + 1e-9 || MathAbs(steps - MathRound(steps)) > 1e-6)
        {
         SendResult(cid, false, TRADE_RETCODE_INVALID_VOLUME, 0, 0, 0, "invalid volume");
         return;
        }
      req.volume = volume;
      req.sl = NormalizeDouble(sl, digits);
      req.tp = NormalizeDouble(tp, digits);
      if(kind == "market")
        {
         MqlTick tick;
         if(!SymbolInfoTick(symbol, tick))
           {
            SendResult(cid, false, TRADE_RETCODE_PRICE_OFF, 0, 0, 0, "no price");
            return;
           }
         double market = (side == "buy" ? tick.ask : tick.bid);
         //--- the engine's risk calculation was made for `price`: refuse if the market moved too far
         if(point > 0 && MathAbs(market - price) / point > deviation + 1e-6)
           {
            SendResult(cid, false, TRADE_RETCODE_REQUOTE, 0, market, 0, "price moved beyond the allowed deviation");
            return;
           }
         req.action = TRADE_ACTION_DEAL;
         req.type = (side == "buy" ? ORDER_TYPE_BUY : ORDER_TYPE_SELL);
         req.price = market;
         req.deviation = (ulong)MathMax(0.0, deviation);
         req.type_filling = FillingFor(symbol);
        }
      else
        {
         req.action = TRADE_ACTION_PENDING;
         if(kind == "limit")
            req.type = (side == "buy" ? ORDER_TYPE_BUY_LIMIT : ORDER_TYPE_SELL_LIMIT);
         else
            req.type = (side == "buy" ? ORDER_TYPE_BUY_STOP : ORDER_TYPE_SELL_STOP);
         req.price = NormalizeDouble(price, digits);
         req.type_time = ORDER_TIME_GTC;
         req.type_filling = ORDER_FILLING_RETURN;
        }
     }
   else if(action == "close")
     {
      if(!PositionSelectByTicket((ulong)ticket))
        {
         SendResult(cid, false, AEGIS_RETCODE_NOT_FOUND, ticket, 0, 0, "position not found");
         return;
        }
      if(volume <= 0 || volume > PositionGetDouble(POSITION_VOLUME) + 1e-9)
        {
         SendResult(cid, false, TRADE_RETCODE_INVALID_VOLUME, ticket, 0, 0, "close volume exceeds position");
         return;
        }
      bool is_buy = (PositionGetInteger(POSITION_TYPE) == POSITION_TYPE_BUY);
      req.action = TRADE_ACTION_DEAL;
      req.position = (ulong)ticket;
      req.symbol = PositionGetString(POSITION_SYMBOL);
      req.volume = volume;
      req.type = (is_buy ? ORDER_TYPE_SELL : ORDER_TYPE_BUY);
      req.price = SymbolInfoDouble(req.symbol, is_buy ? SYMBOL_BID : SYMBOL_ASK);
      req.deviation = 50;
      req.type_filling = FillingFor(req.symbol);
     }
   else if(action == "modify")
     {
      if(!PositionSelectByTicket((ulong)ticket))
        {
         SendResult(cid, false, AEGIS_RETCODE_NOT_FOUND, ticket, 0, 0, "position not found");
         return;
        }
      if(sl <= 0)
        {
         SendResult(cid, false, AEGIS_RETCODE_REFUSED, ticket, 0, 0, "a stop-loss is mandatory");
         return;
        }
      req.action = TRADE_ACTION_SLTP;
      req.position = (ulong)ticket;
      req.symbol = PositionGetString(POSITION_SYMBOL);
      int d = (int)SymbolInfoInteger(req.symbol, SYMBOL_DIGITS);
      req.sl = NormalizeDouble(sl, d);
      req.tp = NormalizeDouble(tp, d);
     }
   else if(action == "cancel")
     {
      if(!OrderSelect((ulong)ticket))
        {
         SendResult(cid, false, AEGIS_RETCODE_NOT_FOUND, ticket, 0, 0, "order not found");
         return;
        }
      req.action = TRADE_ACTION_REMOVE;
      req.order = (ulong)ticket;
     }
   else
     {
      SendResult(cid, false, AEGIS_RETCODE_REFUSED, 0, 0, 0, "unknown action");
      return;
     }

   //--- send to the broker and report exactly what it answered
   ulong t0 = GetMicrosecondCount();
   bool sent = OrderSend(req, res);
   long broker_us = (long)(GetMicrosecondCount() - t0);
   bool ok = sent && (res.retcode == TRADE_RETCODE_DONE || res.retcode == TRADE_RETCODE_DONE_PARTIAL || res.retcode == TRADE_RETCODE_PLACED);
   long out_ticket = ticket;
   if(action == "open")
     {
      out_ticket = (long)res.order;
      //--- on netting accounts the position ticket differs from the order ticket
      if(ok && kind == "market" && !PositionSelectByTicket(res.order) && PositionSelect(symbol))
         out_ticket = PositionGetInteger(POSITION_TICKET);
     }
   string msg = res.comment;
   if(!sent && msg == "")
      msg = "OrderSend failed, error " + IntegerToString(GetLastError());
   SendResult(cid, ok, (long)res.retcode, out_ticket, res.price, res.volume, msg, broker_us);
  }

//+------------------------------------------------------------------+
//| Incoming frames                                                  |
//+------------------------------------------------------------------+
void HandleFrame(const string line)
  {
   string json = AegisOpenFrame(g_key, line);
   if(json == "")
     {
      Disconnect("frame rejected: bad authentication");
      return;
     }
   string sid, type;
   long seq = 0, ts = 0, v = 0;
   if(!AegisJsonLong(json, "v", v) || v != AEGIS_PROTOCOL_VERSION || !AegisJsonString(json, "sid", sid) || sid != g_sid ||
      !AegisJsonLong(json, "seq", seq) || !AegisJsonLong(json, "ts", ts) || !AegisJsonString(json, "t", type))
     {
      Disconnect("frame rejected: bad envelope");
      return;
     }
   //--- strict sequence: a replayed or missing frame closes the session
   if(seq != g_seq_in + 1)
     {
      Disconnect("frame rejected: sequence (replay or loss)");
      return;
     }
   if(MathAbs(NowMs() - ts) > 30000)
     {
      Disconnect("frame rejected: timestamp outside the window (check the PC clock)");
      return;
     }
   g_seq_in = seq;
   g_last_rx_ms = GetTickCount64();

   if(type == "order")
      HandleOrder(json);
   else if(type == "history_req")
     {
      string symbol, tf;
      long count = 0;
      if(AegisJsonString(json, "symbol", symbol) && AegisJsonString(json, "tf", tf) && AegisJsonLong(json, "count", count))
         SendHistory(symbol, tf, (int)MathMin(count, 5000));
     }
   else if(type == "error")
     {
      string msg;
      AegisJsonString(json, "msg", msg);
      Print("AEGIS: the engine reported an error: ", msg);
     }
   else if(type == "bye")
      Disconnect("engine closed the session");
   // heartbeat: nothing to do
  }

void ReadFrames()
  {
   while(g_session)
     {
      uint avail = SocketIsReadable(g_socket);
      if(avail == 0)
         break;
      uchar chunk[];
      int got = SocketRead(g_socket, chunk, (int)MathMin(avail, 4096), 50);
      if(got <= 0)
         break;
      int old = ArraySize(g_rx);
      if(old + got > 4 * AEGIS_MAX_FRAME)
        {
         Disconnect("receive buffer overflow");
         return;
        }
      ArrayResize(g_rx, old + got);
      ArrayCopy(g_rx, chunk, old, 0, got);
     }
   //--- extract complete lines
   while(g_session)
     {
      int n = ArraySize(g_rx);
      int nl = -1;
      for(int i = 0; i < n; i++)
         if(g_rx[i] == '\n')
           {
            nl = i;
            break;
           }
      if(nl < 0)
        {
         if(n > AEGIS_MAX_FRAME)
            Disconnect("frame too large");
         break;
        }
      string line = CharArrayToString(g_rx, 0, nl, CP_UTF8);
      ArrayRemove(g_rx, 0, nl + 1);
      HandleFrame(line);
     }
  }

//+------------------------------------------------------------------+
void OnTimer()
  {
   ulong now = GetTickCount64();
   if(!g_session)
     {
      if(now - g_last_connect_ms < 3000)
         return;
      g_last_connect_ms = now;
      Connect();
      return;
     }
   if(!SocketIsConnected(g_socket))
     {
      Disconnect("connection lost");
      return;
     }
   ReadFrames();
   if(!g_session)
      return;
   if(now - g_last_rx_ms > 10000)
     {
      Disconnect("engine silent for more than 10 s");
      return;
     }
   SendTicks();
   if(now - g_last_account_ms >= 1000)
      SendAccount();
   if(g_snapshot_dirty || now - g_last_snapshot_ms >= 2000)
      SendSnapshot();
   if(now - g_last_heartbeat_ms >= 2000)
     {
      g_last_heartbeat_ms = now;
      UpdateServerOffset();
      SendBody(JStr("t", "heartbeat"));
     }
  }

void OnTick() { /* everything is driven by the timer */ }

//+------------------------------------------------------------------+
//| Broker events: closing deals and balance operations              |
//+------------------------------------------------------------------+
void OnTradeTransaction(const MqlTradeTransaction &trans, const MqlTradeRequest &request, const MqlTradeResult &result)
  {
   g_snapshot_dirty = true;
   if(trans.type != TRADE_TRANSACTION_DEAL_ADD || !g_session)
      return;
   if(!HistoryDealSelect(trans.deal))
      return;
   long deal_type = HistoryDealGetInteger(trans.deal, DEAL_TYPE);
   long time_ms = HistoryDealGetInteger(trans.deal, DEAL_TIME_MSC) - g_server_offset_ms;
   if(deal_type == DEAL_TYPE_BALANCE || deal_type == DEAL_TYPE_CREDIT)
     {
      SendBody(JStr("t", "balance_op") + "," + JInt("deal", (long)trans.deal) + "," +
               JNum("amount", HistoryDealGetDouble(trans.deal, DEAL_PROFIT), 2) + "," + JInt("time", time_ms));
      return;
     }
   long entry = HistoryDealGetInteger(trans.deal, DEAL_ENTRY);
   if(entry != DEAL_ENTRY_OUT && entry != DEAL_ENTRY_INOUT && entry != DEAL_ENTRY_OUT_BY)
      return; // opening deals are reported through the instruction result
   long position_id = HistoryDealGetInteger(trans.deal, DEAL_POSITION_ID);
   string symbol = HistoryDealGetString(trans.deal, DEAL_SYMBOL);
   //--- the closing deal has the opposite direction of the position it closes
   string side = (deal_type == DEAL_TYPE_SELL ? "buy" : "sell");
   double volume = HistoryDealGetDouble(trans.deal, DEAL_VOLUME);
   double price = HistoryDealGetDouble(trans.deal, DEAL_PRICE);
   double profit = HistoryDealGetDouble(trans.deal, DEAL_PROFIT);
   double swap = HistoryDealGetDouble(trans.deal, DEAL_SWAP);
   double commission = HistoryDealGetDouble(trans.deal, DEAL_COMMISSION);
   //--- protocol 1.1: other broker fees on the deal
   double fee = HistoryDealGetDouble(trans.deal, DEAL_FEE);
   string comment = HistoryDealGetString(trans.deal, DEAL_COMMENT);
   //--- comment of the order that OPENED this position (the AEGIS client id), plus its entry commission
   string origin = "";
   if(HistorySelectByPosition(position_id))
     {
      for(int i = 0; i < HistoryDealsTotal(); i++)
        {
         ulong d = HistoryDealGetTicket(i);
         if(d != 0 && HistoryDealGetInteger(d, DEAL_ENTRY) == DEAL_ENTRY_IN)
           {
            if(origin == "")
               origin = HistoryDealGetString(d, DEAL_COMMENT);
            //--- charge the entry commission pro rata to the volume being closed
            double in_volume = HistoryDealGetDouble(d, DEAL_VOLUME);
            if(in_volume > 0)
              {
               commission += HistoryDealGetDouble(d, DEAL_COMMISSION) * MathMin(1.0, volume / in_volume);
               fee += HistoryDealGetDouble(d, DEAL_FEE) * MathMin(1.0, volume / in_volume);
              }
           }
        }
     }
   SendBody(JStr("t", "deal") + "," + JInt("deal", (long)trans.deal) + "," + JInt("position", position_id) + "," +
            JStr("symbol", symbol) + "," + JStr("side", side) + "," + JNum("volume", volume) + "," + JNum("price", price) + "," +
            JNum("profit", profit, 2) + "," + JNum("swap", swap, 2) + "," + JNum("commission", commission, 2) + "," +
            JInt("time", time_ms) + "," + JStr("comment", comment) + "," + JStr("origin", origin) + "," + JNum("fee", fee, 2));
  }
//+------------------------------------------------------------------+
