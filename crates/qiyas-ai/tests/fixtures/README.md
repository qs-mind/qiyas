# Subscription fixtures

`claude.py` is a fake CLI and never calls an AI service.
`fake-oidc-private.der` is a generated, public test signing key with no account or production use.
Its public key is `fake-oidc-jwks.json`. Tests sign synthetic ID tokens only.
All fixture code and generated data use the repository Apache-2.0 license.
