; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-e1edc95f2bc3c53a098bd7f90291d7af705f0b4219ed13e551beeaa04e2aca03-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-e1edc95f2bc3c53a098bd7f90291d7af705f0b4219ed13e551beeaa04e2aca03|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e1edc95f2bc3c53a098bd7f90291d7af705f0b4219ed13e551beeaa04e2aca03|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-e1edc95f2bc3c53a098bd7f90291d7af705f0b4219ed13e551beeaa04e2aca03|COMPLETE" crlf))
