# Test certificates for the goat checks

Certificates only, made with `openssl` for the unit tests of
`flow/src/goat/cert.rs`. They belong to no goatnet and no deployment. The
private keys of all four were deleted when the files were made, so nothing can
be signed or authenticated with them.

| File | Subject common name | Issued by |
|---|---|---|
| `ca-root.pem` | `first-bite test fixture ca` | itself |
| `other-ca-root.pem` | `first-bite test fixture other-ca` | itself |
| `slot-cert.pem` | `e7a9c75b532e3d434bb09fefd3ce1ec7` (the example slot UUID of goat's documentation) | `ca-root.pem` |
| `org-cert.pem` | `example-vendor-trial-peer` | `ca-root.pem` |

All are ECDSA P-256, valid from 2026-11-15 to 2126-10-22; the leaves carry the
extended key usage `clientAuth`. The tests pass the time to check against, so
expiry is tested without an expired file.

To make them again:

```bash
openssl ecparam -name prime256v1 -genkey -noout -out ca.key
openssl req -x509 -new -key ca.key -sha256 -days 36500 -out ca-root.pem \
  -subj "/CN=first-bite test fixture ca" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign"
printf 'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n' > leaf.ext
openssl ecparam -name prime256v1 -genkey -noout -out slot.key
openssl req -new -key slot.key -subj "/CN=e7a9c75b532e3d434bb09fefd3ce1ec7" -out slot.csr
openssl x509 -req -in slot.csr -CA ca-root.pem -CAkey ca.key -CAcreateserial -sha256 \
  -days 36500 -extfile leaf.ext -out slot-cert.pem
rm ./*.key
```
