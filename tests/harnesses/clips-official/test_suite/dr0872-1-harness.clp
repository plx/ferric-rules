; Harness for clips-official/test_suite/dr0872-1.clp
; Detected constructs: deffunction: testUnmatched/0
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-0756247893f01305bd4153a04e4ca3322ad95a8536cf43d4fea93c221612a532-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-0756247893f01305bd4153a04e4ca3322ad95a8536cf43d4fea93c221612a532|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0756247893f01305bd4153a04e4ca3322ad95a8536cf43d4fea93c221612a532|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0756247893f01305bd4153a04e4ca3322ad95a8536cf43d4fea93c221612a532|COMPLETE" crlf))
