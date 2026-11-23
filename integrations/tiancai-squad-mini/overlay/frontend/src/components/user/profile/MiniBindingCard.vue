<template>
  <section class="card p-5">
    <h3 class="font-semibold">微信小程序</h3>
    <p class="mt-2 text-sm">在小程序「天才小分队」中输入绑定码，即可查看本账户和充值。</p>
    <button class="btn btn-primary mt-3" :disabled="busy" @click="issue">生成绑定码</button>
    <p v-if="code" class="mt-3 select-all" aria-live="polite"><strong>{{ code }}</strong>（三分钟内有效）</p>
    <p v-if="message" role="alert">{{ message }}</p>
  </section>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { apiClient } from '@/api/client'

const busy = ref(false)
const code = ref('')
const message = ref('')
async function issue() {
  if (busy.value) return
  busy.value = true
  code.value = ''
  message.value = ''
  try {
    const { data } = await apiClient.post<{code: string}>('/auth/mini/bind-code', {})
    code.value = data.code
  } catch {
    message.value = '绑定码暂不可用，请稍后重试。'
  } finally {
    busy.value = false
  }
}
</script>
