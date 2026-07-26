; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-6832abea04997d33a72beb84f1b3354ca223da2aaf73cf2c8f6691bdbfbb1949-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-6832abea04997d33a72beb84f1b3354ca223da2aaf73cf2c8f6691bdbfbb1949|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6832abea04997d33a72beb84f1b3354ca223da2aaf73cf2c8f6691bdbfbb1949|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-6832abea04997d33a72beb84f1b3354ca223da2aaf73cf2c8f6691bdbfbb1949|COMPLETE" crlf))
