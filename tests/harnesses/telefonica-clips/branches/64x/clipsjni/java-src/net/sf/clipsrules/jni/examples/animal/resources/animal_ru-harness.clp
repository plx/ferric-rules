; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-735900ec4a3497b6a2a8f0a93e0afcfa21ff0082d15869e81d553c393711260e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-735900ec4a3497b6a2a8f0a93e0afcfa21ff0082d15869e81d553c393711260e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-735900ec4a3497b6a2a8f0a93e0afcfa21ff0082d15869e81d553c393711260e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-735900ec4a3497b6a2a8f0a93e0afcfa21ff0082d15869e81d553c393711260e|COMPLETE" crlf))
