// 用 `bevy-agent test` 实时模式测试 BGM（经内建 boa 执行；agent 由运行器注入）。
//
// 前提：游戏已在另一处运行并启用强制失败，例如：
//   BEVY_EMOJI_FORCE_GAME_OVER=1 cargo run --quiet &
// 然后对它跑测试：
//   bevy-agent test scripts/test_music_live.js
//
// 流程：等 BGM 就绪 → 断言游戏结束时音乐暂停 → 按 R 重开 → 断言音乐恢复。
const PROBE = "bevy_emoji::audio::AudioProbe";

agent.wait_for(PROBE, "sink_present", "true", "eq", 40);
agent.wait_for(PROBE, "last_action", "Paused", "eq", 30);
print("[1/2] PASS 游戏结束 → 音乐暂停");

agent.key("KeyR");
agent.wait_for(PROBE, "last_action", "Playing", "eq", 10);
print("[2/2] PASS 重开 → 音乐恢复");

print("ALL PASSED ✅");
