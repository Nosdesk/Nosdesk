# SNS verification test fixtures

Throwaway, self-signed RSA keypair generated solely for `sns.rs` unit tests
(it signs a canonical string in one test and the cert's public key verifies it
in another). Not used by any runtime path and not a real credential. Regenerate
with:

```
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out sns_test_key.pem
openssl req -x509 -new -key sns_test_key.pem -days 3650 -subj "/CN=sns-test-signing-cert" -out sns_test_cert.pem
```
