use crate::proposals::PeerDisconnectProposal;
use crate::{Effects, TezedgeState};
use tla_sm::Acceptor;

impl<'a, Efs> Acceptor<PeerDisconnectProposal<'a, Efs>> for TezedgeState
where
    Efs: Effects,
{
    fn accept(&mut self, proposal: PeerDisconnectProposal<'a, Efs>) {
        if let Err(_err) = self.validate_proposal(&proposal) {
            #[cfg(test)]
            assert_ne!(_err, crate::InvalidProposalError::ProposalOutdated);
            return;
        }

        slog::warn!(&self.log, "Disconnecting peer"; "peer_address" => proposal.peer.to_string(), "reason" => "Requested by the Proposer");
        self.disconnect_peer(proposal.at, proposal.peer);

        self.periodic_react(proposal.at, proposal.effects);
    }
}
