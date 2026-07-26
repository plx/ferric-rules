; Harness for fawkes-robotics/src/plugins/clips-executive/clips/cx-identity.clp
; Detected constructs: deffunction: cx-identity-set/1, cx-identity/0
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ecca735d4d588213cf77ef937d386911c823ef2b1a60ba5a59c91f9a184dee61-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ecca735d4d588213cf77ef937d386911c823ef2b1a60ba5a59c91f9a184dee61|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ecca735d4d588213cf77ef937d386911c823ef2b1a60ba5a59c91f9a184dee61|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ecca735d4d588213cf77ef937d386911c823ef2b1a60ba5a59c91f9a184dee61|COMPLETE" crlf))
