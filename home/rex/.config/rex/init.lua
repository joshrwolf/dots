-- Rex configuration. Each feature installs its own actions and bindings.
require("dots.tabs").install()
require("dots.panes").install()
require("dots.nav").install()
require("dots.views").install()
require("dots.go").install()
require("dots.jumps").install()
require("dots.prs").install()
require("dots.workers").install()
require("dots.goals").install()
require("dots.diff").install()

-- Pi reports its session through the Program Resume Protocol, so a server
-- restart brings it back.
rex.set("resume.trust.pi", { command = { "pi", "--session", rex.match.uuid } })
