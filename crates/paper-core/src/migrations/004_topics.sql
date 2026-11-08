-- Migration 4: the library's topic list. Jev picks a paper's automatic tags
-- from these names, reading each description (docs/research/topic-tags.md).
CREATE TABLE topics (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE,
  description TEXT NOT NULL,
  position INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  modified_at TEXT NOT NULL
);

-- The starting list, the one measured in the research.
INSERT INTO topics (id, name, description, position, created_at, modified_at) VALUES
  ('01a1b925-5bf2-76c3-81fa-05aff0520356', 'Software engineering', 'How software is designed, built, and maintained: architecture, microservices, requirements, code review, refactoring, DevOps.', 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf3-782e-8719-9314337486dd', 'Software testing', 'Testing and debugging programs: unit test generation, fuzzing, mutation testing, coverage, fault localization.', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf4-7855-b4c4-a5f33a59cb15', 'Code generation', 'Models or tools that write, complete, or repair source code.', 2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf5-7be5-9c0c-c14fb1090b53', 'Distributed systems', 'Systems spread over many machines: cloud, microservices, serverless, consensus, parameter servers.', 3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf6-7a10-a80c-8d6f8fb96758', 'Security', 'Attacks on and defenses of computer systems and software, including prompt injection and malware.', 4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf7-752f-a749-e439e5d20a0f', 'Machine learning', 'Classical or general machine learning when no more specific topic fits.', 5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf8-7031-97cd-096650d32000', 'Deep learning', 'Neural network methods: CNNs, transformers, autoencoders, training of deep models.', 6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bf9-75c6-a936-7a10f0851aeb', 'Large language models', 'Language models such as GPT or Llama, their training, prompting, and evaluation.', 7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bfa-789d-8bd3-5b2ffbfe5406', 'AI agents', 'LLM based agents that plan and act in a loop, including software engineering agents. Not agent-based simulation, not RL agents in general.', 8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bfb-70d6-972e-ad2496df0d27', 'Agent harness', 'The software around an agent model: tool definitions, skills, context management, sandboxes, the Model Context Protocol, evaluation harnesses.', 9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bfc-7e72-81bb-24a9bdeb7e15', 'Reinforcement learning', 'Learning from rewards: deep RL, policy optimization, RL from feedback.', 10, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bfd-7a6d-8858-b57c905b477f', 'Computer vision', 'Understanding images and video: classification, detection, segmentation, face analysis.', 11, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bfe-7213-b83b-6d7b08544156', 'ML systems', 'Systems that run machine learning: distributed training, GPU and accelerator use, inference serving, compilers.', 12, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5bff-7f84-98d7-ebaf111ba8ff', 'Edge AI', 'Running models on phones, IoT devices, or embedded hardware, including quantization and energy use for such devices. Not image edge detection.', 13, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c00-74f8-821a-d5f3418c9f0f', 'Physical AI', 'Embodied AI: models that perceive and act in the physical world through robots, world models, vision language action models.', 14, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c01-7794-aa8e-2e9cdfd21310', 'Robotics', 'Robots, manipulation, robot control and planning.', 15, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c02-7574-9456-011e11e2e58a', 'AI safety', 'Alignment, harmful outputs, reward hacking, misuse of AI systems.', 16, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c03-7807-ac23-a76145cf2984', 'Benchmarks', 'Papers whose main contribution is a benchmark or an evaluation method.', 17, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c04-74c8-90c8-df00864b3c6c', 'GNSS', 'Satellite navigation (GPS, Galileo, BeiDou, GLONASS): receivers, signals, positioning.', 18, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c05-76bb-893e-0fc14053f097', 'GNSS spoofing', 'Fake satellite navigation signals and their detection and mitigation. Not face or voice anti-spoofing.', 19, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c06-711a-adba-6e2f1acbad9a', 'GNSS jamming', 'Jamming and radio interference against satellite navigation receivers. Not radar jamming.', 20, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c07-76f6-8381-0363f251d0ff', 'Inertial navigation', 'INS, IMUs, Kalman filters, GNSS/INS and visual-inertial integration.', 21, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c08-7e59-8832-605ea741e428', 'Localization', 'Positioning without GNSS or in addition to it: SLAM, visual odometry, indoor positioning.', 22, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c09-763f-9eb9-ad017c4e9790', 'UAVs', 'Drones and unmanned aerial vehicles.', 23, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c0a-7293-984a-cf325097dc6b', 'Autonomous vehicles', 'Self-driving cars and their perception, planning, and safety.', 24, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c0b-770d-8356-712fed91215c', 'Signal processing', 'Radio, radar, and array signal processing: direction of arrival, beamforming, detection theory.', 25, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ('01a1b925-5c0c-7abf-91b1-c7837109d4ab', 'Wireless and IoT', 'Wireless networks, 5G, and Internet of Things devices and protocols.', 26, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
