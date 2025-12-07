import axios from "axios";

type AddressBytes = number[];

export type TxPayload =
  | { Transfer: { to: AddressBytes; amount: string } }
  | { Stake: { amount: string } }
  | { Delegate: { validator: AddressBytes; amount: string } };

export interface Tx {
  chain_id: string;
  nonce: number;
  gas_limit: number;
  max_fee?: string;
  max_priority_fee?: string;
  gas_price?: string;
  payload: TxPayload;
  signature: string;
}

export async function sendRawTx(endpoint: string, tx: Tx) {
  return axios.post(`${endpoint}/send_raw_tx`, { tx });
}

export function buildTransfer(
  chain_id: string,
  toHex: string,
  amount: string
): Tx {
  return {
    chain_id,
    nonce: 0,
    gas_limit: 21000,
    gas_price: "1",
    payload: { Transfer: { to: hexToBytes32(toHex), amount } },
    signature: "",
  };
}

export function buildStake(
  chain_id: string,
  amount: string,
  gas_price = "1"
): Tx {
  return {
    chain_id,
    nonce: 0,
    gas_limit: 50000,
    gas_price,
    payload: { Stake: { amount } },
    signature: "",
  };
}

export function buildDelegate(
  chain_id: string,
  validatorHex: string,
  amount: string,
  gas_price = "1"
): Tx {
  return {
    chain_id,
    nonce: 0,
    gas_limit: 60000,
    gas_price,
    payload: { Delegate: { validator: hexToBytes32(validatorHex), amount } },
    signature: "",
  };
}

export async function submitToSequencer(apiBase: string, domain_id: string, tx: Tx) {
  return axios.post(`${apiBase}/v1/submit_tx`, { domain_id, tx });
}

export async function getDomainHead(apiBase: string, domain_id: string) {
  const res = await axios.get(`${apiBase}/v1/domain_head`, { params: { domain_id } });
  return res.data as number;
}

export async function getBatchStatus(apiBase: string, domain_id: string, batch_id: string) {
  const res = await axios.get(`${apiBase}/v1/batch_status`, { params: { domain_id, batch_id } });
  return res.data as { batch_id: string; posted: boolean; blob_ref?: { id: string; domain_id: string; size_bytes: number } } | null;
}

function hexToBytes32(hex: string): AddressBytes {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
  const bytes = clean.match(/.{1,2}/g)?.map((b) => parseInt(b, 16)) ?? [];
  const buf = new Array(32).fill(0);
  for (let i = 0; i < Math.min(32, bytes.length); i++) {
    buf[i] = bytes[i];
  }
  return buf;
}
