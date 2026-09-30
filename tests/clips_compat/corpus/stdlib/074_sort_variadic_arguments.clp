;; Sort flattens scalar and multifield arguments and accepts variadic predicates.
;; Level: boundary
;; Covers: +, >, create$, deffunction, nth$, sort
;; Run with load, reset, and run in a fresh environment.

(deffunction exchange ($?args) (> (nth$ 1 ?args) (nth$ 2 ?args)))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort <) " " (sort < (create$)) " " (sort < 7) crlf)
    (printout t (sort > 3 (create$ 1 4) (create$) 2) crlf)
    (printout t (sort + (create$ 3 1 2)) " " (sort exchange (create$ 3 1 2)) crlf))
