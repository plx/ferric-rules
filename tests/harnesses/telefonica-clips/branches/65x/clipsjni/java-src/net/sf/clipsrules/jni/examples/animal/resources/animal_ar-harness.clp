; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-553dd7efc0f9fbf1e034bd59946a7977b94b97b825e72090dd7263ea44be0514-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-553dd7efc0f9fbf1e034bd59946a7977b94b97b825e72090dd7263ea44be0514|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-553dd7efc0f9fbf1e034bd59946a7977b94b97b825e72090dd7263ea44be0514|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-553dd7efc0f9fbf1e034bd59946a7977b94b97b825e72090dd7263ea44be0514|COMPLETE" crlf))
