use crate::tools::Plan;
use crate::ui;
use std::collections::HashSet;

#[derive(Default)]
pub struct Permissions {
    always: HashSet<String>,
    allow_all: bool,
}

pub enum Decision {
    Allow,
    Deny,
    Abort, // user pressed Ctrl-C at the prompt: cancel the whole turn
}

impl Permissions {
    pub fn new(allow_all: bool) -> Self {
        Self {
            always: HashSet::new(),
            allow_all,
        }
    }

    pub async fn check(&mut self, plan: &Plan) -> Decision {
        if self.allow_all {
            return Decision::Allow;
        }
        let rule = plan.always_rule();
        if let Some((key, _)) = &rule {
            if self.always.contains(key) {
                return Decision::Allow;
            }
        }
        match ui::ask(rule.as_ref().map(|(_, desc)| desc.as_str())).await {
            ui::Choice::Yes => Decision::Allow,
            ui::Choice::No => Decision::Deny,
            ui::Choice::Always => {
                if let Some((key, _)) = rule {
                    self.always.insert(key);
                }
                Decision::Allow
            }
            ui::Choice::Abort => Decision::Abort,
        }
    }
}
