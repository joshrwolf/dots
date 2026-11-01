package settings

// Config holds an optional enabled flag, a bounded limit, and independent labels.
type Config struct {
	Enabled *bool
	Limit   int
	Labels  map[string]string
}

func boolPointer(value bool) *bool {
	return &value
}

// New enables the configuration, clamps limit to [1,100], and clones labels.
// A nil labels input remains nil.
func New(limit int, labels map[string]string) Config {
	if limit < 1 {
		limit = 1
	}
	if limit > 100 {
		limit = 100
	}
	var copied map[string]string
	if labels != nil {
		copied = make(map[string]string, len(labels))
		for key, value := range labels {
			copied[key] = value
		}
	}
	return Config{Enabled: boolPointer(true), Limit: limit, Labels: copied}
}
