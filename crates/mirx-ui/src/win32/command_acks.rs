//! The core's answers to the UI's commands, by command.

use std::collections::BTreeSet;

use mirx_core::CommandId;

/// The commands the core has answered, by ID: acks come out of order
/// (lanes), and each command gets exactly one.
#[derive(Debug, Default)]
pub(super) struct AnsweredCommands {
    /// Every ID below this one is answered. IDs start from 1.
    below: u64,
    /// The answered IDs from `below` on.
    above: BTreeSet<u64>,
}

impl AnsweredCommands {
    /// Panics on a second ack of `id`: the core acks each command once.
    pub(super) fn answer(&mut self, id: CommandId) {
        let id = id.get();
        assert!(
            id > self.below && self.above.insert(id),
            "the core acked command {id} twice"
        );
        while self.above.remove(&(self.below + 1)) {
            self.below += 1;
        }
    }

    pub(super) fn is_answered(&self, id: CommandId) -> bool {
        id.get() <= self.below || self.above.contains(&id.get())
    }

    /// How many were answered: `sent()` minus this is what the core owes.
    pub(super) fn count(&self) -> u64 {
        self.below + self.above.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::AnsweredCommands;
    use crate::win32::test_fixtures::inert_command_sender;
    use mirx_core::{Command, CommandId};

    /// IDs 1..=`count`, from a real sender: the type has no other
    /// constructor.
    fn ids(count: usize) -> Vec<CommandId> {
        let (sender, _receiver) = inert_command_sender();
        (0..count)
            .map(|_| sender.send(Command::StartSync).expect("the receiver lives"))
            .collect()
    }

    #[test]
    fn answered_commands_take_acks_in_any_order() {
        let ids = ids(5);
        let mut answered = AnsweredCommands::default();
        for index in [1, 0, 3] {
            answered.answer(ids[index]);
        }
        let answered_ids = |answered: &AnsweredCommands| {
            ids.iter()
                .map(|id| answered.is_answered(*id))
                .collect::<Vec<_>>()
        };
        assert_eq!(answered_ids(&answered), [true, true, false, true, false]);
        assert_eq!(answered.count(), 3);

        answered.answer(ids[2]);
        assert_eq!(answered_ids(&answered), [true, true, true, true, false]);
        assert_eq!(answered.count(), 4);
    }

    #[test]
    #[should_panic(expected = "the core acked command 2 twice")]
    fn a_second_ack_of_a_command_panics() {
        let ids = ids(3);
        let mut answered = AnsweredCommands::default();
        answered.answer(ids[1]);
        answered.answer(ids[1]);
    }

    #[test]
    #[should_panic(expected = "the core acked command 1 twice")]
    fn a_second_ack_of_a_compacted_command_panics() {
        let ids = ids(1);
        let mut answered = AnsweredCommands::default();
        answered.answer(ids[0]);
        answered.answer(ids[0]);
    }
}
