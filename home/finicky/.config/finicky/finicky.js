// Brave profile directories (from brave://version in each profile).
const workProfileDirectory = "Default";
const personalProfileDirectory = "Profile 1";

function braveProfile(profileDirectory) {
  return (url) => ({
    name: "Brave Browser",
    // Supply open's arguments explicitly. Finicky 4.2.2's `profile` lookup
    // can silently omit the profile flag if it cannot read Local State.
    args: ["-n", "--args", `--profile-directory=${profileDirectory}`, url.href],
  });
}

export default {
  defaultBrowser: "Brave Browser",
  options: {
    keepRunning: true,
    logRequests: false,
    checkForUpdates: false, // Homebrew manages updates.
  },
  handlers: [
    {
      match: (url) =>
        url.protocol === "https:" && url.hostname === "oauth2.sigstore.dev",
      browser: braveProfile(workProfileDirectory),
    },
    {
      match: (url) =>
        ["http:", "https:"].includes(url.protocol) &&
        (url.hostname === "github.com" || url.hostname.endsWith(".github.com")),
      browser: braveProfile(workProfileDirectory),
    },
    {
      match: (url) =>
        ["http:", "https:"].includes(url.protocol) &&
        ["youtube.com", "youtu.be", "youtube-nocookie.com", "twitch.tv"].some(
          (domain) =>
            url.hostname === domain || url.hostname.endsWith(`.${domain}`),
        ),
      browser: braveProfile(personalProfileDirectory),
    },
  ],
};
