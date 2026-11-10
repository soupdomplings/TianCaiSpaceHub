<template>
  <main class="hub-connect">
    <section>
      <div class="hub-mark">T</div>
      <h1>{{ flow === 'return' ? '回到天才小分队' : '登录天才 Hub' }}</h1>
      <p role="status">{{ message }}</p>
      <template v-if="flow === 'login' && code && !finished">
        <strong class="code">{{ code }}</strong>
        <p>请确认与 Hub 中的核对码一致</p>
        <button :disabled="busy" @click="approve">以 {{ name }} 继续</button>
        <a href="/profile">切换账号请先在主站退出</a>
      </template>
      <button v-if="failed" @click="retry">重试</button>
      <a v-if="finished" href="/dashboard">进入主站</a>
    </section>
  </main>
</template>
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { apiClient } from '@/api/client'
import { useAuthStore } from '@/stores/auth'

const route = useRoute(), router = useRouter(), auth = useAuthStore()
const params = new URLSearchParams(route.hash.slice(1))
const flow = params.get('flow'), id = params.get('request') || '', verifier = params.get('verifier') || ''
const code = ref(''), message = ref('正在确认登录…'), busy = ref(false), finished = ref(false), failed = ref(false)
const name = computed(() => auth.user?.username || auth.user?.email || '当前账号')
async function begin() {
  failed.value = false; busy.value = true
  try {
    if (!/^[a-f0-9]{64}$/.test(id) || !['login','return'].includes(flow || '')) throw new Error('请从 Hub 重新开始登录')
    if (flow === 'return') {
      // Remove the one-use proof from history before any other navigation or request.
      history.replaceState(history.state, '', '/hub/connect')
      if (!/^[a-f0-9]{64}$/.test(verifier)) throw new Error('接续登录请求无效，请从 Hub 重试')
      const { data } = await apiClient.post('/auth/hub/complete', { id, verifier, purpose: 'return' })
      if (data.state !== 'ready' || !data.access_token || !data.refresh_token || !data.user?.id) throw new Error('登录请求已失效，请从 Hub 重试')
      localStorage.setItem('auth_token', data.access_token)
      localStorage.setItem('refresh_token', data.refresh_token)
      localStorage.setItem('token_expires_at', String(Date.now() + data.expires_in * 1000))
      localStorage.setItem('auth_user', JSON.stringify(data.user))
      // Reload the app to reset account-scoped permissions/caches and refresh scheduling.
      location.replace('/dashboard'); return
    }
    if (!auth.isAuthenticated) {
      await router.replace({ path: '/login', query: { redirect: route.fullPath } }); return
    }
    const { data } = await apiClient.get('/auth/hub/preview', { params: { id } })
    code.value = data.code; message.value = '使用主站账号登录 Hub，余额和权限保持一致。'
  } catch (e) { message.value = e instanceof Error ? e.message : '登录未完成，请重试'; failed.value = true }
  finally { busy.value = false }
}
async function approve() {
  busy.value = true; failed.value = false
  try {
    await apiClient.post('/auth/hub/approve', { id })
    finished.value = true; code.value = ''; message.value = '授权完成，可以回到 Hub 了。'
    history.replaceState(history.state, '', '/hub/connect')
  } catch { message.value = '授权未完成，请重试；若已超时，请在 Hub 重新登录。'; failed.value = true }
  finally { busy.value = false }
}
function retry() { if (flow === 'login' && code.value) void approve(); else void begin() }
onMounted(() => { void begin() })
</script>
<style scoped>
.hub-connect { min-height:100vh; display:grid; place-items:center; background:var(--bg-primary,#f6f7fb); color:var(--text-primary,#263145); padding:24px }
section { width:min(420px,100%); text-align:center; padding:36px; background:var(--bg-secondary,#fff); border-radius:24px; box-shadow:0 16px 64px #213c6b0d }
.hub-mark { margin:auto; width:56px; height:56px; display:grid; place-items:center; border-radius:18px; background:#6874e9; color:white; font-size:28px }
h1 { margin:24px 0 12px; font-size:24px } p { font-size:14px; line-height:1.7; opacity:.75 } .code { display:block; font-size:32px; letter-spacing:.2em; margin:24px 0 }
button { display:block; width:100%; border:0; border-radius:12px; padding:12px 16px; background:#6874e9; color:#fff; cursor:pointer; margin:20px 0 12px } button:disabled { opacity:.5; cursor:wait }
a { display:block; font-size:13px; color:#6874e9; margin-top:20px }
</style>
