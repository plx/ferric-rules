; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-d5350b17dd4b508220e3490e8174480acf0dc1b97ef58375249e1a099c90d79f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-d5350b17dd4b508220e3490e8174480acf0dc1b97ef58375249e1a099c90d79f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d5350b17dd4b508220e3490e8174480acf0dc1b97ef58375249e1a099c90d79f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d5350b17dd4b508220e3490e8174480acf0dc1b97ef58375249e1a099c90d79f|COMPLETE" crlf))
