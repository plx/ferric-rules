; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-2fbae5aca2c99c75131ab3a770e8074e8a939ee529a3f4e742705808fdd93d43-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-2fbae5aca2c99c75131ab3a770e8074e8a939ee529a3f4e742705808fdd93d43|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2fbae5aca2c99c75131ab3a770e8074e8a939ee529a3f4e742705808fdd93d43|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2fbae5aca2c99c75131ab3a770e8074e8a939ee529a3f4e742705808fdd93d43|COMPLETE" crlf))
