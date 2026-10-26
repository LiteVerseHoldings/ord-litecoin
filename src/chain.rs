use {super::*, clap::ValueEnum};

#[derive(Default, ValueEnum, Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Chain {
  #[default]
  #[value(alias("main"))]
  Mainnet,
  #[value(alias("test"))]
  Testnet,
  Signet,
  Regtest,
}

impl Chain {
  pub(crate) fn network(self) -> Network {
    self.into()
  }

  pub(crate) fn default_rpc_port(self) -> u16 {
    match self {
      Self::Mainnet => 9332,
      Self::Regtest => 19443,
      Self::Signet => 39332,
      Self::Testnet => 19332,
    }
  }

  pub(crate) fn inscription_content_size_limit(self) -> Option<usize> {
    match self {
      Self::Mainnet | Self::Regtest => None,
      Self::Testnet | Self::Signet => Some(1024),
    }
  }

  pub(crate) fn first_inscription_height(self) -> u32 {
    match self {
      Self::Mainnet => 2424429,
      Self::Regtest => 0,
      Self::Signet => 112402,
      Self::Testnet => 2669127,
    }
  }

  pub(crate) fn first_rune_height(self) -> u32 {
    Rune::first_rune_height(self.into())
  }

  pub(crate) fn jubilee_height(self) -> u32 {
    match self {
      Self::Mainnet => 2608704,
      Self::Regtest => 110,
      Self::Signet => 3096576,
      Self::Testnet => 3096576,
    }
  }

  pub(crate) fn genesis_block(self) -> Block {
    bitcoin::blockdata::constants::genesis_block(self.network())
  }

  pub(crate) fn genesis_coinbase_outpoint(self) -> OutPoint {
    OutPoint {
      txid: self.genesis_block().coinbase().unwrap().txid(),
      vout: 0,
    }
  }

  pub(crate) fn address_from_script(self, script: &Script) -> Result<Address, SnafuError> {
    Address::from_script(script, self.network()).snafu_context(error::AddressConversion)
  }

  pub(crate) fn join_with_data_dir(self, data_dir: impl AsRef<Path>) -> PathBuf {
    match self {
      Self::Mainnet => data_dir.as_ref().to_owned(),
      Self::Testnet => data_dir.as_ref().join("testnet3"),
      Self::Signet => data_dir.as_ref().join("signet"),
      Self::Regtest => data_dir.as_ref().join("regtest"),
    }
  }
}

impl From<Chain> for Network {
  fn from(chain: Chain) -> Network {
    match chain {
      Chain::Mainnet => Network::Bitcoin,
      Chain::Testnet => Network::Testnet,
      Chain::Signet => Network::Signet,
      Chain::Regtest => Network::Regtest,
    }
  }
}

impl Display for Chain {
  fn fmt(&self, f: &mut Formatter) -> fmt::Result {
    write!(
      f,
      "{}",
      match self {
        Self::Mainnet => "mainnet",
        Self::Regtest => "regtest",
        Self::Signet => "signet",
        Self::Testnet => "testnet",
      }
    )
  }
}

impl FromStr for Chain {
  type Err = SnafuError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "mainnet" => Ok(Self::Mainnet),
      "regtest" => Ok(Self::Regtest),
      "signet" => Ok(Self::Signet),
      "testnet" => Ok(Self::Testnet),
      _ => Err(SnafuError::InvalidChain {
        chain: s.to_string(),
      }),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn litecoin_network_rules_are_preserved() {
    assert_eq!(SUBSIDY_HALVING_INTERVAL, 840_000);
    assert_eq!(DIFFCHANGE_INTERVAL, 2_016);
    assert_eq!(Sat::SUPPLY, 8_399_999_990_760_000);
    assert_eq!(Chain::Mainnet.default_rpc_port(), 9332);
    assert_eq!(Chain::Testnet.default_rpc_port(), 19332);
    assert_eq!(Chain::Regtest.default_rpc_port(), 19443);
    assert_eq!(Chain::Signet.default_rpc_port(), 39332);
    assert_eq!(Chain::Mainnet.first_inscription_height(), 2_424_429);
    assert_eq!(Chain::Mainnet.jubilee_height(), 2_608_704);
    assert_eq!(Chain::Mainnet.first_rune_height(), 2_675_600);

    assert_eq!(
      Chain::Mainnet.genesis_block().block_hash().to_string(),
      "12a765e31ffd4059bada1e25190f6e98c99d9714d334efa41a195a7e7e04bfe2",
    );
    assert_eq!(
      Chain::Mainnet.genesis_coinbase_outpoint().txid.to_string(),
      "97ddfbbae6be97fd6cdf3e7ca13232a3afff2353e29badfab7f73011edd4ced9",
    );
  }

  #[test]
  fn litecoin_mweb_rpc_types_are_recognized() {
    use bitcoincore_rpc::json::ScriptPubkeyType;

    for value in [
      ScriptPubkeyType::Witness_Mweb_Pegin,
      ScriptPubkeyType::Witness_Mweb_HogAddr,
    ] {
      let json = serde_json::to_string(&value).unwrap();
      assert_eq!(
        serde_json::from_str::<ScriptPubkeyType>(&json).unwrap(),
        value
      );
    }
  }

  #[test]
  fn native_segwit_addresses_use_litecoin_prefixes() {
    let script = ScriptBuf::from_bytes([vec![0, 20], vec![0; 20]].concat());

    for (chain, prefix) in [
      (Chain::Mainnet, "ltc1q"),
      (Chain::Testnet, "tltc1q"),
      (Chain::Regtest, "rltc1q"),
    ] {
      assert!(chain
        .address_from_script(&script)
        .unwrap()
        .to_string()
        .starts_with(prefix));
    }
  }

  #[test]
  fn from_str() {
    assert_eq!("mainnet".parse::<Chain>().unwrap(), Chain::Mainnet);
    assert_eq!("regtest".parse::<Chain>().unwrap(), Chain::Regtest);
    assert_eq!("signet".parse::<Chain>().unwrap(), Chain::Signet);
    assert_eq!("testnet".parse::<Chain>().unwrap(), Chain::Testnet);
    assert_eq!(
      "foo".parse::<Chain>().unwrap_err().to_string(),
      "Invalid chain `foo`"
    );
  }
}
