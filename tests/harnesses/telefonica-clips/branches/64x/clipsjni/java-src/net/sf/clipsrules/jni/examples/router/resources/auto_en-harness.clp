; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-7c9e29547c7f88d7be9dc26881d0e3eaefda4d6e55e96470447d62acb7737f29-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-7c9e29547c7f88d7be9dc26881d0e3eaefda4d6e55e96470447d62acb7737f29|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7c9e29547c7f88d7be9dc26881d0e3eaefda4d6e55e96470447d62acb7737f29|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7c9e29547c7f88d7be9dc26881d0e3eaefda4d6e55e96470447d62acb7737f29|COMPLETE" crlf))
