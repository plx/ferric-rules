; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-b6aa0140b0e3453b011c854f086c1ccf3beb1414b3c3d8b3c2a141fb3a6bc0d8-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-b6aa0140b0e3453b011c854f086c1ccf3beb1414b3c3d8b3c2a141fb3a6bc0d8|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b6aa0140b0e3453b011c854f086c1ccf3beb1414b3c3d8b3c2a141fb3a6bc0d8|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b6aa0140b0e3453b011c854f086c1ccf3beb1414b3c3d8b3c2a141fb3a6bc0d8|COMPLETE" crlf))
