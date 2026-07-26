; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AutoWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-3800d47529ce68b65dc469727be4e32c688918092a67b14161af80f984e82cdd-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-3800d47529ce68b65dc469727be4e32c688918092a67b14161af80f984e82cdd|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3800d47529ce68b65dc469727be4e32c688918092a67b14161af80f984e82cdd|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3800d47529ce68b65dc469727be4e32c688918092a67b14161af80f984e82cdd|COMPLETE" crlf))
