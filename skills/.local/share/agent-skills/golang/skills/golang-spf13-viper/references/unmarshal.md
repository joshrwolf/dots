# Viper Unmarshal and Struct Mapping

## Table of Contents

- [Basic Unmarshal](#basic-unmarshal)
- [mapstructure tags](#mapstructure-tags)
- [UnmarshalKey — extracting a sub-tree](#unmarshalkey--extracting-a-sub-tree)
- [time.Duration](#timeduration)
- [net.IP and custom types](#netip-and-custom-types)
- [Squash for embedded structs](#squash-for-embedded-structs)
- [Remain for unknown keys](#remain-for-unknown-keys)
- [Weak decoding](#weak-decoding)

## Basic Unmarshal

```go
type Config struct {
    Port     int    `mapstructure:"port"`
    Host     string `mapstructure:"host"`
    LogLevel string `mapstructure:"log_level"`
    Database struct {
        DSN     string `mapstructure:"dsn"`
        MaxConn int    `mapstructure:"max_conn"`
    } `mapstructure:"database"`
}

var cfg Config
if err := viper.Unmarshal(&cfg); err != nil {
    return fmt.Errorf("decoding config: %w", err)
}
```

## mapstructure tags

Always use `mapstructure` tags. Without them, mapstructure falls back to case-insensitive field name matching, which works for simple cases but silently fails for:

- Nested structs where the outer key uses an underscore (`max_conn` → `MaxConn`)
- Unexported fields (tags cannot make them settable)
- Fields where the Go name does not match the config key

```go
// ✓ Good — explicit and immune to rename surprises
type TLSConfig struct {
    CertFile string `mapstructure:"cert_file"`
    KeyFile  string `mapstructure:"key_file"`
    Enabled  bool   `mapstructure:"enabled"`
}

// ✗ Fragile — relies on case-folding; breaks when config key uses underscores
type TLSConfig struct {
    CertFile string  // viper key "certfile" or "CertFile", not "cert_file"
    KeyFile  string
    Enabled  bool
}
```

## UnmarshalKey — extracting a sub-tree

```go
type DatabaseConfig struct {
    DSN     string `mapstructure:"dsn"`
    MaxConn int    `mapstructure:"max_conn"`
}

var dbCfg DatabaseConfig
if err := viper.UnmarshalKey("database", &dbCfg); err != nil {
    return fmt.Errorf("decoding database config: %w", err)
}
```

Prefer `UnmarshalKey` over `viper.Sub` + `Unmarshal` — fewer nil checks and less boilerplate.

## time.Duration

Viper's `GetDuration` parses duration strings (`"1h30m"`, `"500ms"`) from config files and env vars. Viper already supplies a duration decode hook by default. If this fails, check the Unmarshal error, key binding, and whether a custom hook replaced the default.

When supplying custom hooks, preserve the conversions your config needs:

```go
import "github.com/go-viper/mapstructure/v2"

var cfg Config
err := viper.Unmarshal(&cfg, func(dc *mapstructure.DecoderConfig) {
    dc.DecodeHook = mapstructure.ComposeDecodeHookFunc(
        mapstructure.StringToTimeDurationHookFunc(),
        mapstructure.StringToSliceHookFunc(","),
        dc.DecodeHook,
    )
})
```

`StringToTimeDurationHookFunc` handles `"1h30m"` → `time.Duration`. `StringToSliceHookFunc(",")` handles `"a,b,c"` → `[]string{"a", "b", "c"}`.

## net.IP and custom types

```go
import "github.com/go-viper/mapstructure/v2"

func stringToIPHookFunc() mapstructure.DecodeHookFunc {
    return func(f reflect.Type, t reflect.Type, data any) (any, error) {
        if f.Kind() != reflect.String || t != reflect.TypeOf(net.IP{}) {
            return data, nil
        }
        ip := net.ParseIP(data.(string))
        if ip == nil {
            return nil, fmt.Errorf("invalid IP address: %s", data)
        }
        return ip, nil
    }
}
```

## Squash for embedded structs

```go
type BaseConfig struct {
    LogLevel string `mapstructure:"log_level"`
    Debug    bool   `mapstructure:"debug"`
}

type ServerConfig struct {
    BaseConfig `mapstructure:",squash"`  // merge BaseConfig fields at this level
    Port       int    `mapstructure:"port"`
}
```

Without `,squash`, the base config must be nested under a `baseconfig` key in the config file.

## Remain for unknown keys

```go
type Config struct {
    Port     int            `mapstructure:"port"`
    Remain   map[string]any  `mapstructure:",remain"`
}
```

Extra keys from the config file are collected in `Remain` instead of being silently dropped. Useful for forward compatibility.

## Weak decoding

Viper enables WeaklyTypedInput by default (for example, string `"true"` → bool `true`). To require strict types:

```go
var cfg Config
err := viper.Unmarshal(&cfg, func(dc *mapstructure.DecoderConfig) {
    dc.WeaklyTypedInput = false
})
```

An environment-only key read successfully with GetBool may still be absent from Unmarshal: AutomaticEnv does not register keys. BindEnv("enabled") or a SetDefault registers the key. Missing keys are not decode failures.
