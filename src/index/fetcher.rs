use {
  super::*,
  base64::Engine,
  hyper::{client::HttpConnector, Body, Client, Method, Request, Uri},
  serde_json::{json, Value},
};

pub(crate) struct Fetcher {
  auth: String,
  client: Client<HttpConnector>,
  url: Uri,
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
  };

  #[test]
  fn duplicate_funding_transactions_are_fetched_once_in_original_order() {
    let core = test_bitcoincore_rpc::builder()
      .network(Network::Regtest)
      .build();
    let blocks = core.mine_blocks(2);
    let first = blocks[0].txdata[0].clone();
    let second = blocks[1].txdata[0].clone();
    let fetcher = Fetcher {
      auth: "Basic dXNlcm5hbWU6cGFzc3dvcmQ=".into(),
      client: Client::new(),
      url: core.url().parse().unwrap(),
    };
    let before = core.raw_transaction_requests();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let actual = runtime
      .block_on(fetcher.get_transactions(vec![
        first.txid(),
        second.txid(),
        first.txid(),
        first.txid(),
        second.txid(),
      ]))
      .unwrap();
    assert_eq!(
      actual,
      vec![first.clone(), second.clone(), first.clone(), first, second]
    );
    assert_eq!(core.raw_transaction_requests() - before, 2);
  }

  fn funding_transactions() -> (Transaction, Transaction) {
    let core = test_bitcoincore_rpc::builder()
      .network(Network::Regtest)
      .build();
    let blocks = core.mine_blocks(2);
    (blocks[0].txdata[0].clone(), blocks[1].txdata[0].clone())
  }

  fn one_batch(
    respond: impl FnOnce(Vec<Value>) -> Vec<Value> + Send + 'static,
  ) -> (Fetcher, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
      let mut header = Vec::new();
      while !header.ends_with(b"\r\n\r\n") {
        assert!(header.len() < 16384);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
      }
      let header = String::from_utf8(header).unwrap();
      let size = header
        .lines()
        .find_map(|line| {
          let (name, value) = line.split_once(':')?;
          name
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
      assert!(size < 1024 * 1024);
      let mut body = vec![0; size];
      stream.read_exact(&mut body).unwrap();
      let requests: Vec<Value> = serde_json::from_slice(&body).unwrap();
      let response = serde_json::to_vec(&respond(requests)).unwrap();
      write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).unwrap();
      stream.write_all(&response).unwrap();
    });
    (
      Fetcher {
        auth: "Basic dXNlcm5hbWU6cGFzc3dvcmQ=".into(),
        client: Client::new(),
        url: format!("http://{address}").parse().unwrap(),
      },
      server,
    )
  }

  fn replies(first: &Transaction, second: &Transaction) -> Vec<Value> {
    vec![
      json!({"id":0,"error":null,"result":hex::encode(bitcoin::consensus::serialize(first))}),
      json!({"id":1,"error":null,"result":hex::encode(bitcoin::consensus::serialize(second))}),
    ]
  }

  #[test]
  fn out_of_order_unique_replies_restore_original_input_positions() {
    let (first, second) = funding_transactions();
    let response = replies(&first, &second).into_iter().rev().collect();
    let (fetcher, server) = one_batch(move |requests| {
      assert_eq!(requests.len(), 2);
      assert!(requests
        .iter()
        .all(|request| request["method"] == "getrawtransaction"));
      response
    });
    let actual = tokio::runtime::Runtime::new()
      .unwrap()
      .block_on(fetcher.get_transactions(vec![first.txid(), second.txid(), first.txid()]))
      .unwrap();
    server.join().unwrap();
    assert_eq!(actual, vec![first.clone(), second, first]);
  }

  #[test]
  fn malformed_or_unrelated_batch_replies_refuse_input_values() {
    let (first, second) = funding_transactions();
    for case in 0..7 {
      let mut response = replies(&first, &second);
      match case {
        0 => response[1]["id"] = json!(0),
        1 => {
          response.pop();
        }
        2 => response[1]["id"] = json!(2),
        3 => response[1]["result"] = response[0]["result"].clone(),
        4 => response[1]["result"] = json!("not hexadecimal"),
        5 => {
          response[1]["result"] = Value::Null;
          response[1]["error"] = json!({"code":-5,"message":"synthetic transaction unavailable"});
        }
        6 => response[1]["result"] = Value::Null,
        _ => unreachable!(),
      }
      let (fetcher, server) = one_batch(move |requests| {
        assert_eq!(requests.len(), 2);
        response
      });
      let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(fetcher.get_transactions(vec![first.txid(), second.txid(), first.txid()]));
      server.join().unwrap();
      assert!(
        result.is_err(),
        "Invalid reply case {case} supplied input values"
      );
    }
  }

  #[test]
  fn empty_input_batch_does_not_contact_core() {
    let fetcher = Fetcher {
      auth: String::new(),
      client: Client::new(),
      url: "http://127.0.0.1:0".parse().unwrap(),
    };
    assert!(tokio::runtime::Runtime::new()
      .unwrap()
      .block_on(fetcher.get_transactions(Vec::new()))
      .unwrap()
      .is_empty());
  }
}

#[derive(Deserialize, Debug)]
struct JsonResponse<T> {
  error: Option<JsonError>,
  id: usize,
  result: Option<T>,
}

#[derive(Deserialize, Debug)]
struct JsonError {
  code: i32,
  message: String,
}

impl Fetcher {
  pub(crate) fn new(options: &Options) -> Result<Self> {
    let client = Client::new();

    let url = if options.rpc_url(None).starts_with("http://") {
      options.rpc_url(None)
    } else {
      "http://".to_string() + &options.rpc_url(None)
    };

    let url = Uri::try_from(&url).map_err(|e| anyhow!("Invalid rpc url {url}: {e}"))?;

    let (user, password) = options.auth()?.get_user_pass()?;
    let auth = format!("{}:{}", user.unwrap(), password.unwrap());
    let auth = format!(
      "Basic {}",
      &base64::engine::general_purpose::STANDARD.encode(auth)
    );
    Ok(Fetcher { client, url, auth })
  }

  pub(crate) async fn get_transactions(&self, txids: Vec<Txid>) -> Result<Vec<Transaction>> {
    if txids.is_empty() {
      return Ok(Vec::new());
    }

    // Different missing outpoints may belong to the same funding transaction.
    // Fetch and decode it once, then restore one result per original input.
    let mut unique_txids = Vec::new();
    let mut positions = HashMap::new();
    let mut input_order = Vec::with_capacity(txids.len());
    for txid in txids {
      let position = match positions.get(&txid) {
        Some(&position) => position,
        None => {
          let position = unique_txids.len();
          unique_txids.push(txid);
          positions.insert(txid, position);
          position
        }
      };
      input_order.push(position);
    }

    let mut reqs = Vec::with_capacity(unique_txids.len());
    for (i, txid) in unique_txids.iter().enumerate() {
      let req = json!({
        "jsonrpc": "2.0",
        "id": i, // Associate each response with its requested transaction.
        "method": "getrawtransaction",
        "params": [ txid ]
      });
      reqs.push(req);
    }

    let body = Value::Array(reqs).to_string();

    let results: Vec<JsonResponse<String>>;
    let mut retries = 0;

    loop {
      results = match self.try_get_transactions(body.clone()).await {
        Ok(results) => results,
        Err(error) => {
          if retries >= 5 {
            return Err(anyhow!(
              "failed to fetch raw transactions after 5 retries: {}",
              error
            ));
          }

          log::info!("failed to fetch raw transactions, retrying: {}", error);

          tokio::time::sleep(tokio::time::Duration::from_millis(
            100 * u64::pow(2, retries),
          ))
          .await;
          retries += 1;
          continue;
        }
      };
      break;
    }

    // Return early on any error, because we need all results to proceed
    if let Some(err) = results.iter().find_map(|res| res.error.as_ref()) {
      return Err(anyhow!(
        "failed to fetch raw transaction: code {} message {}",
        err.code,
        err.message
      ));
    }

    ensure!(
      results.len() == unique_txids.len(),
      "Incomplete batched JSON-RPC transaction response"
    );
    let mut by_id: Vec<Option<Transaction>> = vec![None; unique_txids.len()];
    for response in results {
      // Core may return batch replies in any order. Never let a missing,
      // repeated or unrelated reply shift an input's previous-output value.
      let id = response.id;
      ensure!(
        id < by_id.len() && by_id[id].is_none(),
        "Invalid or duplicate batched JSON-RPC response id"
      );
      let raw = response
        .result
        .ok_or_else(|| anyhow!("Missing result for batched JSON-RPC response"))?;
      let bytes = hex::decode(raw)
        .map_err(|e| anyhow!("Result for batched JSON-RPC response not valid hex: {e}"))?;
      let transaction: Transaction = bitcoin::consensus::deserialize(&bytes)
        .map_err(|e| anyhow!("Result for batched JSON-RPC response not valid bitcoin tx: {e}"))?;
      ensure!(
        transaction.txid() == unique_txids[id],
        "Batched JSON-RPC transaction does not match requested transaction id"
      );
      by_id[id] = Some(transaction);
    }

    input_order
      .into_iter()
      .map(|position| {
        by_id[position]
          .clone()
          .ok_or_else(|| anyhow!("Missing requested transaction in batched JSON-RPC response"))
      })
      .collect()
  }

  async fn try_get_transactions(&self, body: String) -> Result<Vec<JsonResponse<String>>> {
    let req = Request::builder()
      .method(Method::POST)
      .uri(&self.url)
      .header(hyper::header::AUTHORIZATION, &self.auth)
      .header(hyper::header::CONTENT_TYPE, "application/json")
      .body(Body::from(body))?;

    let response = self.client.request(req).await?;

    let buf = hyper::body::to_bytes(response).await?;

    let results: Vec<JsonResponse<String>> = match serde_json::from_slice(&buf) {
      Ok(results) => results,
      Err(e) => {
        return Err(anyhow!(
          "failed to parse JSON-RPC response: {e}. response: {response}",
          e = e,
          response = String::from_utf8_lossy(&buf)
        ))
      }
    };

    Ok(results)
  }
}
