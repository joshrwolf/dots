package main

import (
	"fmt"

	"github.com/spf13/viper"
)

// initConfig in root.go handles optional file lookup and environment settings.
// Bind keys used only through Unmarshal: AutomaticEnv alone does not enumerate
// them. The dot/hyphen replacer maps database.max-conn to MYAPP_DATABASE_MAX_CONN.
type Config struct {
	Port     int    `mapstructure:"port"`
	Host     string `mapstructure:"host"`
	LogLevel string `mapstructure:"log-level"`
	Database struct {
		DSN     string `mapstructure:"dsn"`
		MaxConn int    `mapstructure:"max-conn"`
	} `mapstructure:"database"`
}

func loadConfig(v *viper.Viper) (Config, error) {
	for _, key := range []string{"port", "host", "log-level", "database.dsn", "database.max-conn"} {
		if err := v.BindEnv(key); err != nil {
			return Config{}, fmt.Errorf("binding %s: %w", key, err)
		}
	}
	var cfg Config
	if err := v.Unmarshal(&cfg); err != nil {
		return Config{}, fmt.Errorf("unmarshaling config: %w", err)
	}
	return cfg, nil
}

// For long-running configuration reloads, use a separate loader and publish a
// validated Config snapshot under synchronization. Viper WatchConfig mutates its
// own state; concurrently reading that same instance is not safe. See the Viper
// skill's watch-and-reload reference for ownership and lifecycle constraints.
