//+------------------------------------------------------------------+
//| AegisProtocol.mqh — AEGIS PROTOCOL v1, connector side.            |
//|                                                                  |
//| Wire format (one frame per line):                                |
//|   <64 hex: HMAC-SHA256(key, json)> <space> <flat json> \n        |
//| Specification: docs/PROTOCOL.md                                  |
//|                                                                  |
//| STATUS: written against the MQL5 reference; NOT compiled here    |
//| (no MetaEditor on the development machine). Run AegisSelfTest    |
//| first: it checks the cryptography against RFC 4231.              |
//+------------------------------------------------------------------+
#property strict

#define AEGIS_PROTOCOL_VERSION 1
#define AEGIS_MAX_FRAME        16384

//--- bytes and hex ---------------------------------------------------

// UTF-8 bytes of a string, without the terminating zero.
void AegisBytes(const string s, uchar &out[])
  {
   int n = StringToCharArray(s, out, 0, WHOLE_ARRAY, CP_UTF8);
   if(n > 0 && out[n - 1] == 0)
      ArrayResize(out, n - 1);
  }

string AegisHex(const uchar &data[])
  {
   string digits = "0123456789abcdef";
   string out = "";
   int n = ArraySize(data);
   for(int i = 0; i < n; i++)
     {
      out += StringSubstr(digits, data[i] >> 4, 1);
      out += StringSubstr(digits, data[i] & 15, 1);
     }
   return out;
  }

bool AegisSha256(const uchar &data[], uchar &out[])
  {
   uchar unused_key[];
   return CryptEncode(CRYPT_HASH_SHA256, data, unused_key, out) == 32;
  }

// HMAC-SHA256 (RFC 2104) built on the terminal's SHA-256.
bool AegisHmac(const uchar &key[], const uchar &msg[], uchar &out[])
  {
   uchar k[];
   int klen = ArraySize(key);
   if(klen > 64)
     {
      if(!AegisSha256(key, k))
         return false;
     }
   else
     {
      ArrayResize(k, klen);
      if(klen > 0)
         ArrayCopy(k, key, 0, 0, klen);
     }
   int have = ArraySize(k);
   ArrayResize(k, 64);
   for(int i = have; i < 64; i++)
      k[i] = 0;

   int mlen = ArraySize(msg);
   uchar inner[];
   ArrayResize(inner, 64 + mlen);
   for(int i = 0; i < 64; i++)
      inner[i] = (uchar)(k[i] ^ 0x36);
   if(mlen > 0)
      ArrayCopy(inner, msg, 64, 0, mlen);
   uchar inner_hash[];
   if(!AegisSha256(inner, inner_hash))
      return false;

   uchar outer[];
   ArrayResize(outer, 64 + 32);
   for(int i = 0; i < 64; i++)
      outer[i] = (uchar)(k[i] ^ 0x5c);
   ArrayCopy(outer, inner_hash, 64, 0, 32);
   return AegisSha256(outer, out);
  }

string AegisMacHex(const uchar &key[], const string json)
  {
   uchar msg[], mac[];
   AegisBytes(json, msg);
   if(!AegisHmac(key, msg, mac))
      return "";
   return AegisHex(mac);
  }

// Session key = HMAC(psk, "AEGIS1|<client nonce>|<server nonce>") — raw 32 bytes.
bool AegisSessionKey(const uchar &psk[], const string client_nonce, const string server_nonce, uchar &out[])
  {
   uchar msg[];
   AegisBytes("AEGIS1|" + client_nonce + "|" + server_nonce, msg);
   return AegisHmac(psk, msg, out);
  }

string AegisNonce()
  {
   string digits = "0123456789abcdef";
   string out = "";
   // Mix several clocks so two terminals started together still differ.
   MathSrand((int)(GetTickCount64() ^ (ulong)TimeLocal() ^ (ulong)GetMicrosecondCount()));
   for(int i = 0; i < 32; i++)
      out += StringSubstr(digits, MathRand() % 16, 1);
   return out;
  }

//--- flat JSON writer ------------------------------------------------

string AegisEscape(const string s)
  {
   string out = "";
   int n = StringLen(s);
   for(int i = 0; i < n; i++)
     {
      ushort c = StringGetCharacter(s, i);
      if(c == '"')
         out += "\\\"";
      else if(c == '\\')
         out += "\\\\";
      else if(c < 32)
         out += " ";
      else
         out += ShortToString(c);
     }
   return out;
  }

string JStr(const string key, const string value) { return "\"" + key + "\":\"" + AegisEscape(value) + "\""; }
string JInt(const string key, const long value)    { return "\"" + key + "\":" + IntegerToString(value); }
string JBool(const string key, const bool value)   { return "\"" + key + "\":" + (value ? "true" : "false"); }
string JNum(const string key, const double value, const int digits = 8)
  {
   double v = (MathIsValidNumber(value) ? value : 0.0);
   return "\"" + key + "\":" + DoubleToString(v, digits);
  }

//--- flat JSON reader ------------------------------------------------
// The engine only sends flat objects (no nesting), so a key lookup is enough.

bool AegisJsonRaw(const string json, const string key, string &raw, bool &is_string)
  {
   string pattern = "\"" + key + "\":";
   int from = 0;
   int n = StringLen(json);
   while(true)
     {
      int pos = StringFind(json, pattern, from);
      if(pos < 0)
         return false;
      // A real key starts right after '{' or ','.
      ushort before = (pos > 0 ? StringGetCharacter(json, pos - 1) : '{');
      if(before == '{' || before == ',')
        {
         int i = pos + StringLen(pattern);
         if(i >= n)
            return false;
         if(StringGetCharacter(json, i) == '"')
           {
            is_string = true;
            raw = "";
            i++;
            while(i < n)
              {
               ushort c = StringGetCharacter(json, i);
               if(c == '\\' && i + 1 < n)
                 {
                  ushort e = StringGetCharacter(json, i + 1);
                  if(e == 'n' || e == 't' || e == 'r')
                     raw += " ";
                  else
                     raw += ShortToString(e);
                  i += 2;
                  continue;
                 }
               if(c == '"')
                  return true;
               raw += ShortToString(c);
               i++;
              }
            return false;
           }
         is_string = false;
         int end = i;
         while(end < n)
           {
            ushort c = StringGetCharacter(json, end);
            if(c == ',' || c == '}')
               break;
            end++;
           }
         raw = StringSubstr(json, i, end - i);
         return true;
        }
      from = pos + 1;
     }
   return false;
  }

bool AegisJsonString(const string json, const string key, string &value)
  {
   bool is_string = false;
   return AegisJsonRaw(json, key, value, is_string) && is_string;
  }

bool AegisJsonNumber(const string json, const string key, double &value)
  {
   string raw;
   bool is_string = false;
   if(!AegisJsonRaw(json, key, raw, is_string) || is_string)
      return false;
   value = StringToDouble(raw);
   return MathIsValidNumber(value);
  }

bool AegisJsonLong(const string json, const string key, long &value)
  {
   string raw;
   bool is_string = false;
   if(!AegisJsonRaw(json, key, raw, is_string) || is_string)
      return false;
   value = StringToInteger(raw);
   return true;
  }

//--- frames ----------------------------------------------------------

// Split "<mac> <json>" and verify the MAC. Returns the JSON, or "" when rejected.
string AegisOpenFrame(const uchar &key[], const string line)
  {
   int n = StringLen(line);
   if(n < 66 || n > AEGIS_MAX_FRAME || StringGetCharacter(line, 64) != ' ')
      return "";
   string mac = StringSubstr(line, 0, 64);
   string json = StringSubstr(line, 65);
   string expected = AegisMacHex(key, json);
   if(StringLen(expected) != 64)
      return "";
   // Compare every character (no early exit).
   int diff = 0;
   for(int i = 0; i < 64; i++)
      diff |= (int)(StringGetCharacter(mac, i) ^ StringGetCharacter(expected, i));
   return (diff == 0 ? json : "");
  }

// Build "<mac> <json>\n" for an envelope.
string AegisSealFrame(const uchar &key[], const string id, const long ts_ms, const long seq, const string sid, const string body_fields)
  {
   string json = "{" + JInt("v", AEGIS_PROTOCOL_VERSION) + "," + JStr("id", id) + "," + JInt("ts", ts_ms) + "," +
                 JInt("seq", seq) + "," + JStr("sid", sid) + "," + body_fields + "}";
   return AegisMacHex(key, json) + " " + json + "\n";
  }
//+------------------------------------------------------------------+
