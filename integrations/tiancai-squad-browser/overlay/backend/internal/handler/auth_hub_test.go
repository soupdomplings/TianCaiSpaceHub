package handler

import (
    "context"
    "encoding/json"
    "strings"
    "testing"
    "time"
    "github.com/alicebob/miniredis/v2"
    "github.com/redis/go-redis/v9"
    "github.com/stretchr/testify/require"
)

func TestHubProofConsumeAndApproval(t *testing.T) {
    server := miniredis.RunT(t)
    cache := redis.NewClient(&redis.Options{Addr:server.Addr()})
    t.Cleanup(func(){ _ = cache.Close() })
    ctx := context.Background()
    key := hubKey(strings.Repeat("a",64))
    r := hubRecord{Challenge:"proof",Code:"123456",Purpose:"login"}
    raw,_ := json.Marshal(r)
    require.NoError(t,cache.Set(ctx,key,raw,3*time.Minute).Err())
    complete := func(proof,purpose string) string {
        value,err := hubComplete.Run(ctx,cache,[]string{key},proof,purpose).Text()
        require.NoError(t,err); return value
    }
    require.Empty(t,complete("wrong","login"))
    require.Empty(t,complete("proof","return"))
    require.Equal(t,"pending",complete("proof","login"))
    require.True(t,server.Exists(key))
    value,err := hubApprove.Run(ctx,cache,[]string{key},int64(7),"version").Int()
    require.NoError(t,err); require.Equal(t,1,value)
    value,err = hubApprove.Run(ctx,cache,[]string{key},int64(8),"other").Int()
    require.NoError(t,err); require.Zero(t,value)
    require.LessOrEqual(t,server.TTL(key),3*time.Minute)
    require.NotEmpty(t,complete("proof","login"))
    require.Empty(t,complete("proof","login"))
    require.False(t,server.Exists(key))
    require.NoError(t,cache.Set(ctx,key,raw,time.Second).Err())
    server.FastForward(2*time.Second)
    require.Empty(t,complete("proof","login"))
}
