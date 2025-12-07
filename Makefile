include .env

get-reth:
	brew install paradigmxyz/brew/reth

run:
	chmod +x run-reth.sh
	RPC_URL=${RPC_URL} ./run-reth.sh
