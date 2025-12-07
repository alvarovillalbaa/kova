import httpx
from typing import List


def build_transfer(chain_id: str, to_hex: str, amount: int) -> dict:
    return {
        "chain_id": chain_id,
        "nonce": 0,
        "gas_limit": 21000,
        "gas_price": "1",
        "payload": {"Transfer": {"to": _hex_to_bytes32(to_hex), "amount": str(amount)}},
        "signature": "",
    }


def build_stake(chain_id: str, amount: int, gas_price: str = "1") -> dict:
    return {
        "chain_id": chain_id,
        "nonce": 0,
        "gas_limit": 50000,
        "gas_price": gas_price,
        "payload": {"Stake": {"amount": str(amount)}},
        "signature": "",
    }


def build_delegate(chain_id: str, validator_hex: str, amount: int, gas_price: str = "1") -> dict:
    return {
        "chain_id": chain_id,
        "nonce": 0,
        "gas_limit": 60000,
        "gas_price": gas_price,
        "payload": {"Delegate": {"validator": _hex_to_bytes32(validator_hex), "amount": str(amount)}},
        "signature": "",
    }


async def send_raw_tx(endpoint: str, tx: dict) -> dict:
    async with httpx.AsyncClient() as client:
        resp = await client.post(f"{endpoint}/send_raw_tx", json=tx)
        resp.raise_for_status()
        return resp.json()


def _hex_to_bytes32(value: str) -> List[int]:
    clean = value[2:] if value.startswith("0x") else value
    by = bytes.fromhex(clean)
    padded = by[:32].ljust(32, b"\x00")
    return list(padded)

