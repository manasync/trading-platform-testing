System Requirements

1. Market Service
- GET /symbols
- GET /prices
- GET /prices/{symbol}
- Prices can be mocked (random or static)
2. Portfolio Service
- GET /portfolio/{userId}
- POST /orders
- GET /orders/{orderId}
Order Logic (market orders only):
- BUY: fetch price → check balance → deduct cash → add asset  
- SELL: check asset → deduct asset → add cash  
- No order book or matching engine required
3. Optional: Audit Service
- Capture events such as:
  - ORDER_CREATED
  - ORDER_EXECUTED
  - ORDER_REJECTED
AI Agent Requirement (Mandatory)
- The candidate must use an AI coding agent (e.g., Cursor, Copilot, etc.)
- Provide an AI_USAGE.md including:
  - Tools used
  - Key prompts
  - Tasks delegated to AI
  - What was accepted vs. modified
  - At least one example of incorrect AI output and how it was handled
Testing Requirements
- Successful BUY / SELL
- Insufficient balance scenarios
- Market service failure handling
- Correct portfolio updates after trades
Deliverables
- Source code (Rust)
- README with setup and architecture
- AI_USAGE.md
- Docker Compose (or equivalent)
- Test instructions