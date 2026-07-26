; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-92f9e56856e6cf5d87d0a023242d253326751b8857b51f5df978b950f22f1d53-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-92f9e56856e6cf5d87d0a023242d253326751b8857b51f5df978b950f22f1d53|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-92f9e56856e6cf5d87d0a023242d253326751b8857b51f5df978b950f22f1d53|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-92f9e56856e6cf5d87d0a023242d253326751b8857b51f5df978b950f22f1d53|COMPLETE" crlf))
