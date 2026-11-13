use {
  super::*,
  ord::subcommand::wallet::{cardinals::CardinalUtxo, outputs::Output},
};

#[test]
fn cardinals() {
  let rpc_server = test_bitcoincore_rpc::spawn();
  create_wallet(&rpc_server);

  inscribe(&rpc_server);

  let all_outputs = CommandBuilder::new("wallet outputs")
    .rpc_server(&rpc_server)
    .run_and_deserialize_output::<Vec<Output>>();

  let cardinal_outputs = CommandBuilder::new("wallet cardinals")
    .rpc_server(&rpc_server)
    .run_and_deserialize_output::<Vec<CardinalUtxo>>();

  assert_eq!(all_outputs.len() - cardinal_outputs.len(), 1);
}

#[test]
fn locked_outputs_are_not_offered_as_cardinal_funding() {
  let rpc_server = test_bitcoincore_rpc::spawn();
  create_wallet(&rpc_server);
  let blocks = rpc_server.mine_blocks(2);
  let reserved = OutPoint::new(blocks[0].txdata[0].txid(), 0);
  let available = OutPoint::new(blocks[1].txdata[0].txid(), 0);
  rpc_server.lock(reserved);

  let outputs = CommandBuilder::new("wallet cardinals")
    .rpc_server(&rpc_server)
    .run_and_deserialize_output::<Vec<CardinalUtxo>>();

  assert_eq!(outputs.len(), 1);
  assert_eq!(outputs[0].output, available);
}
