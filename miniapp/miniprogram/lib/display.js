// Match the target Hub's displayContent protocol; legacy phone storage is ignored.
const defaults = { reasoning: false, tool: false, fileChange: false, artifacts: false };
function fromHub(value) {
  return {
    reasoning: value?.reasoning === true,
    tool: value?.tool === true,
    fileChange: value?.fileChange === true,
    artifacts: value?.artifact === true
  };
}
module.exports = { defaults, fromHub };
