; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-2dbc290a74b72877d4344703b66129442616ed1bbd8049eb154852c78a5b81c0-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-2dbc290a74b72877d4344703b66129442616ed1bbd8049eb154852c78a5b81c0|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2dbc290a74b72877d4344703b66129442616ed1bbd8049eb154852c78a5b81c0|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2dbc290a74b72877d4344703b66129442616ed1bbd8049eb154852c78a5b81c0|COMPLETE" crlf))
