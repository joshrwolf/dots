# Viper Env Binding and Flag Binding

## Table of Contents

- [The binding interaction model](#the-binding-interaction-model)
- [AutomaticEnv vs BindEnv](#automaticenv-vs-bindenv)
- [SetEnvKeyReplacer in depth](#setenvkeyreplacer-in-depth)
- [AllowEmptyEnv](#allowemptyenv)
- [Flag binding](#flag-binding)
- [How pflag binding interacts with precedence](#how-pflag-binding-interacts-with-precedence)
- [Debugging binding](#debugging-binding)

## The binding interaction model

Three settings control how viper maps environment variables to keys. Configure them to match the chosen environment naming scheme; neither prefix nor replacer is mandatory:

```go
viper.SetEnvPrefix("MYAPP")                           // adds MYAPP_ prefix
viper.SetEnvKeyReplacer(strings.NewReplacer(".", "_")) // database.host → MYAPP_DATABASE_HOST
viper.AutomaticEnv()                                   // activates auto-binding
```

Call these before resolving configuration with `viper.Get*` or `Unmarshal` — typically in a root command's `PersistentPreRunE` or in `init()`.

## AutomaticEnv vs BindEnv

| Method | Behavior |
| --- | --- |
| `AutomaticEnv()` | Every key is automatically mapped to its env equivalent (with prefix and replacer applied) |
| `BindEnv(key, envVars...)` | Only the specified key is bound, to the specified env var name(s) |

Use `AutomaticEnv` for the common case. Use `BindEnv` when you need to bind to an env var with a name that doesn't follow your prefix/replacer convention (e.g., third-party env vars like `GOOGLE_APPLICATION_CREDENTIALS`).

```go
// Bind a specific non-prefixed env var
viper.BindEnv("google.credentials", "GOOGLE_APPLICATION_CREDENTIALS")
```

## SetEnvKeyReplacer in depth

Viper keys use `.` as separator for nested values. Shell assignment syntax usually disallows dots, though process environments can contain them. The replacer maps between them.

```go
// Config file:
// database:
//   host: localhost
//   max_conn: 25

// Without replacer:
viper.SetEnvPrefix("MYAPP")
viper.AutomaticEnv()
viper.GetString("database.host")  // looks for MYAPP_DATABASE.HOST — no match

// With replacer:
viper.SetEnvPrefix("MYAPP")
viper.SetEnvKeyReplacer(strings.NewReplacer(".", "_"))
viper.AutomaticEnv()
viper.GetString("database.host")  // looks for MYAPP_DATABASE_HOST — matches
```

For automatic lookup Viper uppercases and prefixes the key first, then applies the replacer to the full environment name. A replacer can therefore affect the prefix too.

## AllowEmptyEnv

By default, viper ignores env vars set to the empty string — the empty string is treated as "not set" and viper continues down the precedence stack. Override this behavior:

```go
viper.AllowEmptyEnv(true)
// now MYAPP_PORT="" → viper.GetInt("port") == 0, not the default
```

## Flag binding

Bind a pflag after defining it:

```go
func init() {
    rootCmd.PersistentFlags().Int("port", 8080, "listen port")
    viper.BindPFlag("port", rootCmd.PersistentFlags().Lookup("port"))
}
```

Bind an entire flag set:

```go
viper.BindPFlags(rootCmd.PersistentFlags())
```

**Timing rule:** Bind before Get/Unmarshal needs the value. Binding after parsing, including in RunE, works: the binding reads the live flag state. Bind during command construction when configuration is resolved in an earlier hook.

## How pflag binding interacts with precedence

Viper checks `flag.Changed` (whether the user explicitly passed the flag). This is how it distinguishes between "flag default" (low priority) and "flag explicitly set" (high priority):

- `flag.Changed == false` (flag has its default): viper treats the flag as not present and falls through to env/file/default.
- `flag.Changed == true` (flag was provided on the command line): viper treats the flag value as higher priority than env/file/default, but below explicit `Set`.

This means `viper.GetInt("port")` correctly returns the flag value when `--port 9090` is passed, and falls back to env `MYAPP_PORT` or config file `port: 8080` otherwise.

## Debugging binding

Inspect non-sensitive resolved values to verify binding. `AllSettings` does not enumerate arbitrary environment variables:

```go
fmt.Println(viper.AllSettings())
// map[database:map[host:localhost max_conn:25] port:8080]
```

Check env var resolution:

```go
os.Setenv("MYAPP_PORT", "9090")
viper.SetEnvPrefix("MYAPP")
viper.AutomaticEnv()
fmt.Println(viper.GetInt("port"))  // 9090
```
