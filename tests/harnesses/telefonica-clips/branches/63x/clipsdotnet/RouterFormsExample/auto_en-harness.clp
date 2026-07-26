; Harness for telefonica-clips/branches/63x/clipsdotnet/RouterFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ba5bcb552d4e2704d37c4cbff1673d026b082a2f1506c3ea316ef277b4e42619-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ba5bcb552d4e2704d37c4cbff1673d026b082a2f1506c3ea316ef277b4e42619|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ba5bcb552d4e2704d37c4cbff1673d026b082a2f1506c3ea316ef277b4e42619|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ba5bcb552d4e2704d37c4cbff1673d026b082a2f1506c3ea316ef277b4e42619|COMPLETE" crlf))
