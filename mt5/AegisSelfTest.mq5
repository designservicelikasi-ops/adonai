//+------------------------------------------------------------------+
//| AegisSelfTest.mq5 — run this SCRIPT once after compiling.         |
//| It checks the connector's cryptography and JSON handling against  |
//| known answers, without any network or trading activity.           |
//|                                                                  |
//| Expected output in the Experts tab: "AEGIS self-test: ALL PASSED" |
//+------------------------------------------------------------------+
#property script_show_inputs
#property strict
#include "AegisProtocol.mqh"

int g_failures = 0;

void Check(const string name, const bool ok)
  {
   Print(ok ? "  ok    " : "  FAIL  ", name);
   if(!ok)
      g_failures++;
  }

void OnStart()
  {
   Print("AEGIS self-test");

   //--- HMAC-SHA256, RFC 4231 test case 2
   uchar key[], msg[], mac[];
   AegisBytes("Jefe", key);
   AegisBytes("what do ya want for nothing?", msg);
   Check("HMAC-SHA256 (RFC 4231 #2)",
         AegisHmac(key, msg, mac) && AegisHex(mac) == "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");

   //--- RFC 4231 test case 1 (20 bytes of 0x0b, "Hi There")
   uchar key1[];
   ArrayResize(key1, 20);
   ArrayInitialize(key1, 0x0b);
   AegisBytes("Hi There", msg);
   Check("HMAC-SHA256 (RFC 4231 #1)",
         AegisHmac(key1, msg, mac) && AegisHex(mac) == "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");

   //--- key longer than the block size (RFC 4231 test case 6: 131 bytes of 0xaa)
   uchar key6[];
   ArrayResize(key6, 131);
   ArrayInitialize(key6, 0xaa);
   AegisBytes("Test Using Larger Than Block-Size Key - Hash Key First", msg);
   Check("HMAC-SHA256 (RFC 4231 #6, long key)",
         AegisHmac(key6, msg, mac) && AegisHex(mac) == "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54");

   //--- frame round trip and tamper detection
   uchar psk[];
   AegisBytes("0123456789abcdef0123456789abcdef", psk);
   string frame = AegisSealFrame(psk, "id-1", 1800000000000, 7, "sid-1", JStr("t", "tick") + "," + JStr("symbol", "EURUSD") + "," + JNum("bid", 1.1, 5));
   string line = StringSubstr(frame, 0, StringLen(frame) - 1);
   string json = AegisOpenFrame(psk, line);
   Check("frame is accepted with the right key", json != "");
   string tampered = line;
   StringReplace(tampered, "1.10000", "9.90000");
   Check("tampered frame is rejected", AegisOpenFrame(psk, tampered) == "");
   uchar other[];
   AegisBytes("another-key-another-key-another!", other);
   Check("frame is rejected with another key", AegisOpenFrame(other, line) == "");
   Check("short line is rejected", AegisOpenFrame(psk, "hello") == "");

   //--- flat JSON reader
   string sample = "{\"v\":1,\"id\":\"x\",\"ts\":1800000000000,\"seq\":12,\"sid\":\"s-1\",\"t\":\"order\",\"cid\":\"Aabc\",\"volume\":0.23,\"msg\":\"say \\\"hi\\\", ok\",\"ticket\":0}";
   string s;
   double d;
   long l;
   Check("string field", AegisJsonString(sample, "cid", s) && s == "Aabc");
   Check("escaped string field", AegisJsonString(sample, "msg", s) && s == "say \"hi\", ok");
   Check("number field", AegisJsonNumber(sample, "volume", d) && MathAbs(d - 0.23) < 1e-12);
   Check("integer field", AegisJsonLong(sample, "ts", l) && l == 1800000000000);
   Check("last field", AegisJsonLong(sample, "ticket", l) && l == 0);
   Check("missing field", !AegisJsonString(sample, "nope", s));
   Check("key inside a value is not a key", !AegisJsonNumber("{\"msg\":\"\\\"volume\\\":5\",\"x\":1}", "volume", d));
   Check("type mismatch is refused", !AegisJsonNumber(sample, "cid", d));

   //--- session key derivation is deterministic and depends on both nonces
   uchar k1[], k2[], k3[];
   AegisSessionKey(psk, "client-nonce-0001", "server-nonce-0001", k1);
   AegisSessionKey(psk, "client-nonce-0001", "server-nonce-0001", k2);
   AegisSessionKey(psk, "client-nonce-0002", "server-nonce-0001", k3);
   Check("session key is deterministic", AegisHex(k1) == AegisHex(k2) && ArraySize(k1) == 32);
   Check("session key depends on the nonces", AegisHex(k1) != AegisHex(k3));
   //--- value computed by the Rust implementation for the same inputs (see docs/PROTOCOL.md)
   Check("session key matches the engine", AegisHex(k1) == "cc3d94318cb6f7e1cedf0f13814703c6318b92b837e70c87a5906e6c525d84ca");

   Print(g_failures == 0 ? "AEGIS self-test: ALL PASSED" : "AEGIS self-test: " + IntegerToString(g_failures) + " FAILURE(S) — do not use the connector");
  }
//+------------------------------------------------------------------+
