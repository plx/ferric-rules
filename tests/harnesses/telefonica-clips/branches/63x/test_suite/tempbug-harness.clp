; Harness for telefonica-clips/branches/63x/test_suite/tempbug.clp
; Detected constructs: defglobal: ?*q*, ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-02caadc3328f15f790d8140662e0964be4ba9d67d7ae5ee16334cfbd95711c1f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-02caadc3328f15f790d8140662e0964be4ba9d67d7ae5ee16334cfbd95711c1f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-02caadc3328f15f790d8140662e0964be4ba9d67d7ae5ee16334cfbd95711c1f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-02caadc3328f15f790d8140662e0964be4ba9d67d7ae5ee16334cfbd95711c1f|COMPLETE" crlf))
