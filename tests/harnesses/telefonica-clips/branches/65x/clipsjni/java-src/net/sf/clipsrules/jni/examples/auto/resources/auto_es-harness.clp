; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-2da8429e31f37d58ff6bebd35dc1e31699c492a0f0490ceb450482bfde3c0d5d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-2da8429e31f37d58ff6bebd35dc1e31699c492a0f0490ceb450482bfde3c0d5d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2da8429e31f37d58ff6bebd35dc1e31699c492a0f0490ceb450482bfde3c0d5d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2da8429e31f37d58ff6bebd35dc1e31699c492a0f0490ceb450482bfde3c0d5d|COMPLETE" crlf))
