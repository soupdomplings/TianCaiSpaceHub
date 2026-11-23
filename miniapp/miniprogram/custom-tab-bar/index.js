Component({
  data: { selected: 0 },
  methods: {
    go(e) {
      const i = Number(e.currentTarget.dataset.i);
      wx.switchTab({ url: ['/pages/devices/index', '/pages/squad/index'][i] });
    }
  }
});
