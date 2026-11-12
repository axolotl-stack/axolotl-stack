//! `/packstack <server|client> [verbose] [exclude-vanilla]`.
//!
//! Measured on BDS 1.26.51 (`tools/bds-sweep/command_probe.py`,
//! `testdata/bds-command-probe.json`), a dedicated server without added
//! packs: one success message per pack, `<name> (<version>)\n`, and with
//! `verbose` a block per pack (`Pack Name`, `UUID`, `Type`, `Version`, each
//! on its own line, then a blank line). Without `verbose` a pack listed in
//! several versions (BDS's vanilla server pack, once per base game version)
//! appears once, with its first version. Every pack in both stacks is
//! vanilla, so `exclude-vanilla` answers `No packs found`.
//!
//! unastar lists BDS's stacks ([`unastar_data::pack_stack`]): the packs it
//! loads itself are not listed.

use crate::command::prelude::*;
use unastar_data::pack_stack::Pack;

arg_enum! {
    /// Captured `stackType`.
    pub enum StackType in "stackType" {
        Server = "server",
        Client = "client",
    }
}

overloads! {
    pub enum PackStackArgs for "packstack" {
        0 => List {
            stack: StackType = "stackType",
            verbose: Option<EnumValue>,
            exclude_vanilla: Option<EnumValue> = "exclude-vanilla",
        },
    }
}

pub fn run(_ctx: &mut CommandContext<'_>, args: PackStackArgs) -> CommandResult {
    let PackStackArgs::List {
        stack,
        verbose,
        exclude_vanilla,
    } = args;
    if exclude_vanilla.is_some() {
        return success!("No packs found");
    }
    let stacks = unastar_data::pack_stack::vanilla();
    let packs = match stack {
        StackType::Server => &stacks.server,
        StackType::Client => &stacks.client,
    };
    let mut output = CommandOutput::default();
    if verbose.is_some() {
        for pack in packs {
            output.add_success(verbose_line(pack), vec![]);
        }
    } else {
        let mut seen: Vec<&str> = Vec::new();
        for pack in packs {
            if seen.contains(&pack.uuid.as_str()) {
                continue;
            }
            seen.push(&pack.uuid);
            output.add_success(format!("{} ({})\n", pack.name, pack.version), vec![]);
        }
    }
    Ok(output)
}

fn verbose_line(pack: &Pack) -> String {
    format!(
        "Pack Name: {}\n  UUID: {}\n  Type: {}\n  Version: {}\n\n",
        pack.name, pack.uuid, pack.kind, pack.version
    )
}

#[cfg(test)]
mod tests {
    use crate::command::exec::evidence::console_records;
    use crate::command::exec::probe::{Probe, captured_console, captured_player};
    use crate::command::{CommandSource, execute};

    #[test]
    fn packstack_answers_like_bds() {
        let mut probe = Probe::new();
        for command in [
            "packstack resource",
            "packstack server",
            "packstack client",
            "packstack server verbose",
            "packstack server verbose exclude-vanilla",
        ] {
            let output = execute(probe.h.world(), CommandSource::server(), command);
            assert_eq!(
                console_records(&output),
                captured_console(command),
                "{command}"
            );
        }
        let player = probe.player();
        for command in ["/packstack server", "/packstack client verbose"] {
            let output = probe.h.run(player, command);
            assert_eq!(output.messages, captured_player(command), "{command}");
            assert_eq!(output.success_count, output.messages.len() as u32);
        }
    }
}
