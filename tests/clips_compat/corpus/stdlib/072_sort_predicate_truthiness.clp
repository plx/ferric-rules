;; Only the symbol FALSE keeps a sort pair in order; other results exchange it.
;; Level: boundary
;; Covers: create$, deffunction, sort
;; Run with load, reset, and run in a fresh environment.

(deffunction keep (?a ?b) FALSE)

(deffunction true-result (?a ?b) TRUE)

(deffunction zero (?a ?b) 0)

(deffunction float-zero (?a ?b) 0.0)

(deffunction nil-result (?a ?b) nil)

(deffunction string-false (?a ?b) "FALSE")

(deffunction empty (?a ?b) (create$))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort keep (create$ 3 1 2)) " "
                (sort true-result (create$ 3 1 2)) " "
                (sort zero (create$ 3 1 2)) " "
                (sort float-zero (create$ 3 1 2)) " "
                (sort nil-result (create$ 3 1 2)) " "
                (sort string-false (create$ 3 1 2)) " "
                (sort empty (create$ 3 1 2)) crlf))
