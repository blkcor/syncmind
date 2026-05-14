package crypto

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"fmt"
)

// KeyPair holds an Ed25519 key pair.
type KeyPair struct {
	PublicKey  ed25519.PublicKey
	PrivateKey ed25519.PrivateKey
}

// GenerateKeyPair generates a new Ed25519 key pair.
func GenerateKeyPair() (*KeyPair, error) {
	_, priv, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		return nil, fmt.Errorf("failed to generate ed25519 key pair: %w", err)
	}
	return &KeyPair{
		PublicKey:  priv.Public().(ed25519.PublicKey),
		PrivateKey: priv,
	}, nil
}

// PublicKeyBase64 returns the base64-encoded public key.
func (k *KeyPair) PublicKeyBase64() string {
	return base64.RawURLEncoding.EncodeToString(k.PublicKey)
}

// PrivateKeyBase64 returns the base64-encoded private key.
func (k *KeyPair) PrivateKeyBase64() string {
	return base64.RawURLEncoding.EncodeToString(k.PrivateKey)
}

// ParseKeyPair decodes base64-encoded keys into a KeyPair.
func ParseKeyPair(pubKeyB64, privKeyB64 string) (*KeyPair, error) {
	pubKey, err := base64.RawURLEncoding.DecodeString(pubKeyB64)
	if err != nil {
		return nil, fmt.Errorf("invalid public key: %w", err)
	}
	privKey, err := base64.RawURLEncoding.DecodeString(privKeyB64)
	if err != nil {
		return nil, fmt.Errorf("invalid private key: %w", err)
	}
	return &KeyPair{
		PublicKey:  ed25519.PublicKey(pubKey),
		PrivateKey: ed25519.PrivateKey(privKey),
	}, nil
}
