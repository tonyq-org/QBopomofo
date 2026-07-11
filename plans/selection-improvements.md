# 注音選字策略改進計畫

本文件記錄對 QBopomofo 選字策略（candidate selection）的可行改進項目。
每項皆經過效能評估，符合 `CLAUDE.md` 的效能紅線（按鍵到候選字 < 5ms、目標 < 2ms）。

## 背景：目前架構

- **核心演算法**（`base/engine/src/conversion/chewing.rs`）：DAG + Dijkstra 找最短路，Yen's algorithm 取前 10 條替代路徑。
- **Scoring**：`log_prob = log(freq/1M) + 長度懲罰`（`chewing.rs:198-206`），純 unigram 模型。
- **使用者學習**（`editor/estimate.rs`）：選詞後 freq +10/+5/+1，SQLite 使用者詞庫與系統詞庫 `max()` 合併。
- 已知 TODO：`chewing.rs:47` 標註 `// TODO: Reranking`。

---

## Tier 1：低成本、零效能風險、立刻有感

### T1-A：修正 Yen's 的 spur candidate 排序

**問題位置**：`base/engine/src/conversion/chewing.rs:324`

```rust
candidates.sort_unstable_by_key(|k| k.len());
```

目前用「邊數量」排序挑 k-th path，經典 Yen's 應按「路徑總 cost」排序。結果是輸出的 top-K 不保證真的是 top-K，可能漏掉低 cost 但多段的路徑。最終 `chewing.rs:48` 會再依 `total_probability` 排一次，輸出穩定性尚可，但已犧牲真實前 K 名。

**改動**：
```rust
candidates.sort_unstable_by(|a, b|
    path_cost(a, phrases).total_cmp(&path_cost(b, phrases))
);
```

**成本**：排序鍵從 `usize` 變 `f64`；K ≤ 10，差異可忽略。
**風險**：極低。有既有測試覆蓋（`convert_cycle_alternatives`、`convert_pathological_case`）。
**預期效益**：ambiguous 輸入下，次要候選更合理。

---

### T1-B：最近選擇 LRU 鎖定（deterministic user override）

**動機**：使用者最常見的抱怨是「我明明剛選過，下次怎麼又跑預設字」。目前 `estimate.rs` 只把 freq +10，若原本落差太大仍翻不過來。

**設計**：
- 在 `Editor` 或 `SharedState` 持有一個 in-memory LRU：
  ```rust
  struct RecentSelections {
      map: LruCache<Box<[Syllable]>, Box<str>>,  // 容量 64
  }
  ```
- 使用者在 selection window 選定非預設候選時寫入。
- `convert()` 回傳後、送出給 UI 前做一次 O(K × intervals) 線性掃描：任一 path 的某個 interval 命中 LRU 就將那條 path 提前到第一。
- 不持久化（程序重啟失效），避免污染長期學習；需要永久記憶仍靠 SQLite user dict。

**位置建議**：
- LRU 結構放 `base/engine/src/editor/recent.rs`（新檔）。
- 寫入點：`editor/mod.rs` 選字確認的地方（`1401-1425` 附近）。
- 讀取點：`conversion/chewing.rs::convert()` 尾端，或在 `editor/mod.rs` 拿到 `Vec<Outcome>` 後重排。

**成本**：
- 寫入：O(1)，每次選字一次。
- 讀取：O(K × avg_intervals) ≈ 50 次雜湊查詢 ≈ <10μs。
- 記憶體：64 條 entry × ~32B ≈ 2KB。

**風險**：低。行為是疊加在現有 scoring 之上的 override，可加 config 開關。
**預期效益**：最有感的 UX 改善，成本最低。

---

### T1-C：tsi.csv 同音詞頻率 spread

**問題位置**：`data-provider/build.sh:99-119`

目前「spread apart homophones with freq diff < 100」只套在 `word.csv`（單字），`tsi.csv`（多字詞）未處理。結果：多字詞同頻碰撞時順序由 Trie 讀取序決定，實務上會出現「兩個詞交替顯示」的不穩定。

**改動**：把 build.sh 裡的 spread 邏輯抽成 Python function，對 `MERGED_TSI` 也跑一次（依注音序列 group，而非單字）。

**成本**：純 build-time，runtime 零影響。
**風險**：極低。
**預期效益**：多字候選排序穩定化。

---

### T1-D：使用者詞頻衰減

**問題位置**：`base/engine/src/editor/estimate.rs:57-84`

目前 `estimate()` 只有「多久前用過」的增量（+10/+5/+1），**沒有任何衰減**。一年前高頻選過的冷門詞會永遠壓在系統常用字之上。

**改動方向（擇一）**：
1. **Soft decay**：delta_time 超過某閾值（如 500000 tick）時回傳 `phrase.freq().saturating_sub(1)`，讓冷掉的詞每被動訪問一次就掉 1。
2. **Persist-time exponential decay**：使用者詞庫載入時，對 `delta_time > LONG_THRESHOLD` 的 entry 套用 `freq *= 0.95`，每次啟動 amortize 一次。

方案 2 衝擊更小且確定性高。

**成本**：方案 2 僅啟動時多幾毫秒（離線）；方案 1 runtime 每次 lookup 多一次 saturating_sub，可忽略。
**風險**：中。會改變長期 ranking。建議加 config flag，預設關閉，觀察一段時間再打開。
**預期效益**：習慣改變後能自然復原，減少「髒使用者詞庫」問題。

---

## Tier 2：中等成本、品質顯著提升、需效能驗證

### T2-E：Build-time bigram 表 + K-best 重排

**動機**：這是解 `chewing.rs:47 TODO: Reranking` 最實際的方案，直接處理「他/她」「再/在」「的/得/地」這類純靠上下文才能消歧的場景。

**設計**：
- **Build-time**：
  - 從 LGPL 相容語料（ASBC 中研院平衡語料庫、維基中文 dump 等）統計 phrase-to-phrase bigram。
  - 編譯成 mmap-friendly 格式：perfect hash / FST / 壓縮後的排序陣列。
  - 僅保留 top-N（例如 top 1M bigram pairs），控制檔案大小在 10-30MB。
- **Runtime**：
  - `find_k_paths` 回傳 10 條 path 後，各自計算 `Σ log P(phrase_i | phrase_{i-1})`。
  - 與 unigram score 線性組合：`total = α × unigram + (1-α) × bigram`。
  - α 可調，初期設 0.7 保守一些。
  - 重排後回傳。

**位置**：
- 新增 crate：`base/engine/src/conversion/rerank.rs`。
- 資料：`data-provider/output/bigram.dat`（mmap 讀取）。
- 語料處理：`data-provider/tools/build-bigram/`（Rust 或 Python）。

**成本估算**：
- 10 path × 平均 5 segment × 1 次雜湊查詢 ≈ 50 次 lookup。
- 每次雜湊查詢 <1μs（mmap + open addressing），總計 <100μs。
- 在 5ms 紅線內綽綽有餘。

**資料大小**：
- 假設 tsi.csv 有 ~160K phrase，bigram 組合保留 top 1M 配對，加壓縮後約 10-20MB。
- mmap 不進駐 RAM，OS page cache 管理。

**風險**：
- 語料來源必須 LGPL 相容 — ASBC 是 CC BY-NC-SA，**不能用於商業 fork**，需另尋來源（或純用 libchewing-data 自己的 tsi.csv 共現）。
- 新增 build 依賴，複雜度上升。

**預期效益**：最顯著的中文選字品質躍升。值得做，但要在 Tier 1 全部落地後再啟動。

---

### T2-F：已提交上下文納入第一段 bigram

**動機**：使用者打長句時，每次 Enter 後下一輪輸入是「冷啟動」，但實際上前一個詞是很強的上下文訊號。

**設計**：
- `Editor` 維護 `last_committed_phrase: Option<Box<str>>`。
- 每次 Enter / commit 後更新。
- `convert()` 做 rerank 時，路徑第一個 segment 的 bigram 分數用 `P(seg_0 | last_committed_phrase)`。
- 若無（剛啟動、剛切換 app），退化回原本 unigram。

**成本**：幾乎免費（每次 convert 多一次 lookup）。
**風險**：低。需定義清楚何時 reset（切換視窗？跨行？）。
**預期效益**：長句連續打字品質明顯提升。

**前置依賴**：T2-E（需要 bigram 表）。

---

### T2-G：Selection 視窗排序與 path 排序一致化

> 2026-07-11 Windows 先落地可切換的「智慧排序」：長詞優先、同長度依詞頻，並去除重複候選；設定頁仍可切回純詞頻或傳統字典順序。這是保守版本，尚未把 path 的完整 probability score 直接套到 selection window。

**問題位置**：
- Path 排序：`conversion/chewing.rs:213`（依 `log(freq) + length_penalty`）
- 候選視窗排序：`editor/selection/phrase.rs:257-259`（依 `phrase.freq()` 降序）

長詞 case 下，使用者可能看到「engine 選 A，但視窗第一排顯示 B」。

**改動**：視窗排序也改用 `log_prob`（或 merged score with 使用者 freq），讓兩處一致。

**成本**：低。
**風險**：中。會改變使用者熟悉的候選字順序。建議用 A/B 開關，預設維持現狀，讓進階使用者試。
**預期效益**：消除「引擎和候選視窗矛盾」的困惑。

---

## Tier 3：不建議做（至少目前不做）

### T3-H：神經 LM reranker

2ms 預算放不下。即使 INT8 量化 + ONNX 也很勉強。除非允許第一下打字先用 unigram、bigram 跑在背景、第二下才用神經分數（但這會違反「候選顯示延遲 = 使用者感知延遲」的原則）。

### T3-I：從使用者改選反推 discriminative learning

概念好，但需要 correction log 基礎建設（目前 repo 沒有），且會觸及隱私議題。工程量大，留到所有 Tier 1/2 都做完再評估。

### T3-J：動態調整 length penalty 常數

`chewing.rs:198-206` 那張表是 tsi.csv 的 corpus statistic，**不該當調校旋鈕用**。要改 ranking 品質，加新訊號（LRU、bigram），不要扭現有訊號。

---

## 建議執行順序

1. **Week 1**：T1-A（Yen's 修正，正確性 bug）、T1-B（LRU override，最高 CP 值）
2. **Week 2**：T1-C（tsi.csv spread）、T1-D（使用者詞頻衰減，加 config flag）
3. **Month 2+**：評估 T2-E（bigram 表）的語料來源與授權，決定是否投入。
4. **Month 3+**：T2-F、T2-G。

Tier 1 四項完成後，預期能解決 70% 以上的使用者選字抱怨，且完全不觸碰效能紅線。

---

## 效能紅線檢查清單

每項變更合入前都要回答：

1. 這段程式碼會在使用者按鍵時執行嗎？
2. 若會，增加了多少延遲？（用 `criterion` benchmark）
3. 有沒有辦法移到 build-time 或非 hot path？

**通過標準**：p99 end-to-end latency 仍 < 5ms，目標 < 2ms。

---

## 相關檔案索引

| 項目 | 檔案路徑 |
|------|---------|
| 主選字演算法 | `base/engine/src/conversion/chewing.rs` |
| 使用者詞頻估算 | `base/engine/src/editor/estimate.rs` |
| 分層字典 | `base/engine/src/dictionary/layered.rs` |
| 候選視窗邏輯 | `base/engine/src/editor/selection/phrase.rs` |
| 編輯器主迴圈 | `base/engine/src/editor/mod.rs` |
| Build pipeline | `data-provider/build.sh` |
