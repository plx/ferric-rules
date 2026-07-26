; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-422f3c6a805259852f3a6eb8ba6959caf9f26c1ca50ae8360b8b032f9cec9820-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-422f3c6a805259852f3a6eb8ba6959caf9f26c1ca50ae8360b8b032f9cec9820|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-422f3c6a805259852f3a6eb8ba6959caf9f26c1ca50ae8360b8b032f9cec9820|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-422f3c6a805259852f3a6eb8ba6959caf9f26c1ca50ae8360b8b032f9cec9820|COMPLETE" crlf))
