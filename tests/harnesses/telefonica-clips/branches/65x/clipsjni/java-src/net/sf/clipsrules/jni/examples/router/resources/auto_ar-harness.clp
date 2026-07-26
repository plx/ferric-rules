; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-5a10cd07132bfb2f6f56ed6edf9bba2ba7717495dc8e2386eb7be765afa3edc2-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-5a10cd07132bfb2f6f56ed6edf9bba2ba7717495dc8e2386eb7be765afa3edc2|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5a10cd07132bfb2f6f56ed6edf9bba2ba7717495dc8e2386eb7be765afa3edc2|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5a10cd07132bfb2f6f56ed6edf9bba2ba7717495dc8e2386eb7be765afa3edc2|COMPLETE" crlf))
