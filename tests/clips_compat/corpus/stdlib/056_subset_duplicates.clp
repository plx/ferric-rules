;; Subset is membership-based: repeated query values do not require repeated targets.
;; Level: boundary
;; Covers: create$, subsetp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (subsetp (create$ a a) (create$ a b)) " " (subsetp (create$) (create$)) " " (subsetp (create$ a c) (create$ a b)) crlf))
