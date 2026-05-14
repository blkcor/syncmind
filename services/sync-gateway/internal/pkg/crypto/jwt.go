package crypto

import (
	"crypto/ed25519"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
)

// SignDeviceJWT creates an Ed25519-signed JWT for a device.
func SignDeviceJWT(privateKey ed25519.PrivateKey, deviceID uuid.UUID, issuer, audience string, ttl time.Duration) (string, error) {
	now := time.Now().UTC()
	token := jwt.NewWithClaims(jwt.SigningMethodEdDSA, jwt.MapClaims{
		"sub": deviceID.String(),
		"iss": issuer,
		"aud": audience,
		"iat": now.Unix(),
		"exp": now.Add(ttl).Unix(),
		"jti": uuid.New().String(),
	})
	return token.SignedString(privateKey)
}
