//! A throwaway RSA key, used only by these tests.
//!
//! Generated once and committed on purpose: signing a JWT needs a key, and
//! a 2048-bit keygen per test run costs seconds for no benefit. This key
//! signs nothing outside this file and is worthless to anyone who has it.

/// PKCS#8 private key, for minting test tokens.
pub const TEST_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCbBrbsaSy3UsiF
/zXD/t7WtsGx0FnKSMsuVBn2ZQegDVd2nzylFOBWqUY8lff8qk6E5Vmi9Dqw7nKJ
ExUazsSM7N8K618HcP58fG+KkESQpSt0t/HUSUTWnsMfoAYixcppSEnyGeMlZggX
facB3jr7FvRpUOYvJEonMgeZP3fuKjMteM22kW1FR7zMCI3bgXCGC8BPNfJPPpJ1
w+4CRF5fOOPx2BXonYzfnJ3A78GvZUGvYkKE12kCZSWrzw2bFznH222ENvVl8ZDr
KQ11SeBcrVIyvVJfBGvMKyZV0xTw/M790Cf2GxN9a3rSwYP+i93RofDm3JsZQ90+
PWE0A/r7AgMBAAECggEAESW3/ARSJuDoPzIQb2J0oYlLeXCTTfWpS7GPwZpBMqnn
H77TRWs+uTc2BqOL742i6742oPikuUdWseTDc9ilEvVsYlfQhhEwhPJ6n/f/LYSn
ftyNyi5kh9y9tnTL2PLJxcVyMG4+mrdjc725SFKdcYKDfFavb01zSkyVXJURne0u
7+LZHyXdkXJo3YkS0NPedJMg+7m+n4BzMkmThofUnUVjRgF7r6tj2/JyY70piv98
cIN8eybHaYeTLiLdMOBQeqeWWWC7MSryRCmQnpplCS/de4+MLP0tu7BLMlKk3C2C
590L4ajNkPElGpY5lER8gmMeoQG7nlLFA5VN4BFuUQKBgQDMk13izlDCdZn2ZG4H
hTy93XpVRTBbl/3A9ow5VdiU138uJM/bUwqaFIrpyFW+SDITV9urL/v1HTyDLrGo
LrM+YKTHAPMq5BKvi7TSW0UVrbdxt1w76ax4SdBKK6U0nOULiG+n0KAelphZei6t
uZmUp6xkexOeG6l2SmzPkCLWqQKBgQDB/svhwr+QmKiSmWZ1LWJsXZK9imb5zTd4
cPVR25bv8xVnkFV5XozbA6IhXv8KSK/B9aJ0fPdU8rhgvfO2chgBp7JcCMDkica+
p3e8GijTdNXJeglEobjmutKwSA++9j9HPdz13McEhR4jXlF/e4GRSDO88I67O1IH
hKqfydUfAwKBgFjHh1H7SS7qzFMSSHG5D6Ax8nn42cCWGEhadoYXTDNjxcynqxC6
W/p7+cD08MjwGdMtKKaE32oDMxSW+gBLq/vhAwFd1ymA6t8F2QYFF9kNl3OhKETT
5sYY+myFvl8zy26S2inQrvw3TIxgKsu3pP/POFAu3VebF5K/P7NgEM5pAoGALU3c
vT9mz1TnYT0T0V+k8Zu0rjEJNWM4hhcTI2e9yxGguQva+jobePZTQanWs8cfzJMZ
ukyI0jzQ1D7oEH56nsBUBexBZ93JHTMs4i/VwvQxDRlD2tRNwwx0MZjSnI0TYAbR
eFVz4NlZnXbkX3ovWwdaldAz9QO4d2sDEcfnzpMCgYAN1pLSTGlRulCtWRO7M4Zb
pcEb0Kq1HZum732fMyV4+KPfS9Ss7V0y+BRZdggm0dVDZSSkuT4pntJLgfVUUfZT
lt5QNhc/mYhL+PMrHssjzIlUuE1/rHSLMeParr/9RTx0pgTNmgqVuAUbGLhSePhG
NIL8lV6nPAMJ3mf7n/IPQA==
-----END PRIVATE KEY-----"#;

/// The same key's modulus, base64url, for the JWKS document.
pub const TEST_KEY_N: &str = "mwa27Gkst1LIhf81w_7e1rbBsdBZykjLLlQZ9mUHoA1Xdp88pRTgVqlGPJX3_KpOhOVZovQ6sO5yiRMVGs7EjOzfCutfB3D-fHxvipBEkKUrdLfx1ElE1p7DH6AGIsXKaUhJ8hnjJWYIF32nAd46-xb0aVDmLyRKJzIHmT937iozLXjNtpFtRUe8zAiN24FwhgvATzXyTz6SdcPuAkReXzjj8dgV6J2M35ydwO_Br2VBr2JChNdpAmUlq88Nmxc5x9tthDb1ZfGQ6ykNdUngXK1SMr1SXwRrzCsmVdMU8PzO_dAn9hsTfWt60sGD_ovd0aHw5tybGUPdPj1hNAP6-w";

/// 65537, the exponent every RSA key here uses.
pub const TEST_KEY_E: &str = "AQAB";

/// The key id the mock provider publishes and signs with.
pub const TEST_KID: &str = "test-key-1";
