;; String-to-field reads the first token and preserves its parsed type.
;; Level: basic
;; Covers: floatp, integerp, string-to-field, symbolp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (string-to-field "42 trailing") " " (integerp (string-to-field "42")) " " (floatp (string-to-field "2.5")) " " (symbolp (string-to-field "red")) crlf))
