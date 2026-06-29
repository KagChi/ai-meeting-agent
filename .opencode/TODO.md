# Meeting Agent — Todo List

**Sync Strategy:** Bidirectional sync with extern repo  
**Extern Source:** `/Users/kagchi/Documents/projects/bmw-ntust-internship/docs/daily-logs/08_MeetingAgent.md`

---

## Phase 1: Repository Setup ✅
- [x] Clone repository
- [x] Review project structure
- [x] Read architecture documentation
- [x] Identify transcription engine location
- [x] Map audio capture pipeline

## Phase 2: Transcription Architecture Analysis ✅
- [x] Locate all transcription implementations
- [x] Identify provider abstraction/interface
- [x] Map dependencies on local models
- [x] Understand audio preparation flow
- [x] Document current OpenAI API integration
- [x] Identify files/modules for local transcription

## Phase 3: Remove Local Transcription Dependencies
- [ ] Remove Whisper.cpp dependencies from Cargo.toml
- [ ] Remove Parakeet/ONNX dependencies
- [ ] Remove GPU acceleration code (CUDA, Metal, Vulkan)
- [ ] Remove model download and management code
- [ ] Remove local model storage and caching
- [ ] Clean up build scripts
- [ ] Update UI to remove local model selection

## Phase 4: Implement Cloud-First Transcription
- [ ] Create unified OpenAI-compatible API client
- [ ] Add configuration for API endpoint
- [ ] Implement audio streaming to API
- [ ] Handle API response parsing
- [ ] Add retry logic and connection handling
- [ ] Implement request queuing

## Phase 5: Configuration and UI Updates
- [ ] Design UI for API endpoint configuration
- [ ] Add settings for API endpoint URL
- [ ] Add secure credential storage
- [ ] Remove local model management UI
- [ ] Add API status indicators
- [ ] Add cost/usage tracking UI (optional)

## Phase 6: Testing and Validation
- [ ] Test with OpenAI-compatible endpoint
- [ ] Benchmark performance
- [ ] Test real-time meeting scenarios
- [ ] Test error handling
- [ ] Test audio format compatibility
- [ ] Measure network latency

## Phase 7: Advanced Features (Deferred)
- [ ] Automatic language detection
- [ ] Speaker identification (if supported)
- [ ] Client-side audio preprocessing
- [ ] Webhook support for async jobs
- [ ] Fallback mechanism if API fails
