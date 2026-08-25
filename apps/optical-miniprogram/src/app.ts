App({
  onLaunch() {
    if (!wx.canIUse("getUpdateManager")) return;
    const updateManager = wx.getUpdateManager();
    updateManager.onUpdateReady(() => {
      wx.showModal({
        title: "新版本已就绪",
        content: "是否立即重启并使用最新版？",
        success: ({ confirm }) => {
          if (confirm) updateManager.applyUpdate();
        },
      });
    });
  },
});
