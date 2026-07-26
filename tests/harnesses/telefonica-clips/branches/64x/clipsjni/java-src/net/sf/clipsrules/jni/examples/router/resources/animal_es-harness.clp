; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-d9d2e1ddf98b4392df5b73053bc2fe889aacb95d873f4899c8d13743b11d5204-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-d9d2e1ddf98b4392df5b73053bc2fe889aacb95d873f4899c8d13743b11d5204|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d9d2e1ddf98b4392df5b73053bc2fe889aacb95d873f4899c8d13743b11d5204|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d9d2e1ddf98b4392df5b73053bc2fe889aacb95d873f4899c8d13743b11d5204|COMPLETE" crlf))
