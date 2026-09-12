;; Case conversion preserves STRING versus SYMBOL input type.
;; Level: boundary
;; Covers: lowcase, stringp, symbolp, upcase
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (upcase "aBc") " " (lowcase XYZ) " " (stringp (upcase "abc")) " " (symbolp (lowcase ABC)) crlf))
