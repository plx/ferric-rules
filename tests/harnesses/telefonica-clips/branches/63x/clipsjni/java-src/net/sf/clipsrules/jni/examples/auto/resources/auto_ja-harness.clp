; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-efed706649b9448345efd6abaa152e640f34664cdae419be727c4f1e7e2b24bd-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-efed706649b9448345efd6abaa152e640f34664cdae419be727c4f1e7e2b24bd|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-efed706649b9448345efd6abaa152e640f34664cdae419be727c4f1e7e2b24bd|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-efed706649b9448345efd6abaa152e640f34664cdae419be727c4f1e7e2b24bd|COMPLETE" crlf))
