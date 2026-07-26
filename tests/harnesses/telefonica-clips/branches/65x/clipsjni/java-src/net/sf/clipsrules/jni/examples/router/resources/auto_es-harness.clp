; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-896835146ae2e81101355486040165b5670699fc9a5f7e1a6eccc4fcc76f3955-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-896835146ae2e81101355486040165b5670699fc9a5f7e1a6eccc4fcc76f3955|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-896835146ae2e81101355486040165b5670699fc9a5f7e1a6eccc4fcc76f3955|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-896835146ae2e81101355486040165b5670699fc9a5f7e1a6eccc4fcc76f3955|COMPLETE" crlf))
