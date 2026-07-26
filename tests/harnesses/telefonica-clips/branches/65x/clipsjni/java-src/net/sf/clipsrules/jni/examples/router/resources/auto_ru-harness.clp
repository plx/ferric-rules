; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-018dd489d44c2565c8991e8c7f860b6481abe77817d13bdc68acac4bebf0c007-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-018dd489d44c2565c8991e8c7f860b6481abe77817d13bdc68acac4bebf0c007|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-018dd489d44c2565c8991e8c7f860b6481abe77817d13bdc68acac4bebf0c007|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-018dd489d44c2565c8991e8c7f860b6481abe77817d13bdc68acac4bebf0c007|COMPLETE" crlf))
