package haiai

import (
	"os"
)

// discoverConfigPath returns the first existing jacs config path, or empty string.
func discoverConfigPath() string {
	candidates := []string{
		os.Getenv("JACS_CONFIG_PATH"),
		"./jacs.config.json",
	}
	home, err := os.UserHomeDir()
	if err == nil {
		candidates = append(candidates, home+"/.jacs/jacs.config.json")
	}
	for _, p := range candidates {
		if p == "" {
			continue
		}
		if _, err := os.Stat(p); err == nil {
			return p
		}
	}
	return ""
}
