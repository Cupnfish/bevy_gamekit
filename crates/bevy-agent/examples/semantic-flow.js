agent
  .id("js-semantic-flow")
  .hover("game.drop", 8)
  .click("game.drop")
  .waitFrames(45)
  .dragBy("board.score", 120, 0, { frames: 12 })
  .key("Space");
