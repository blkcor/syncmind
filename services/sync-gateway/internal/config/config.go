package config

import (
	"github.com/zeromicro/go-zero/core/conf"
)

// Config defines the full configuration schema for the Spine service.
type Config struct {
	DatabaseURL         string `json:"database_url"`
	RedisAddr           string `json:"redis_addr"`
	BindAddr            string `json:"bind_addr"`
	TLSCert             string `json:"tls_cert"`
	TLSKey              string `json:"tls_key"`
	PairingSessionTTL   int    `json:"pairing_session_ttl"`
	BundleRetentionDays int    `json:"bundle_retention_days"`
	MaxBundleSizeMB     int    `json:"max_bundle_size_mb"`
	JWTIssuer           string `json:"jwt_issuer"`
	JWTAudience         string `json:"jwt_audience"`
}

// Load reads configuration from the given YAML path and environment variables.
func Load(path string) (*Config, error) {
	var c Config
	if err := conf.Load(path, &c); err != nil {
		return nil, err
	}
	return &c, nil
}
