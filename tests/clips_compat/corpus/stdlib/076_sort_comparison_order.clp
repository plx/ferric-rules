;; Sort calls its predicate in CLIPS merge-sort order.
;; Level: interaction
;; Covers: >, bind, create$, defglobal, deffunction, implode$, sort, str-cat
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*calls* = "")

(deffunction exchange (?a ?b)
    (bind ?*calls* (str-cat ?*calls* ?a ":" ?b ";"))
    (> ?a ?b))

(deffunction traced (?items)
    (bind ?*calls* "")
    (bind ?items (sort exchange ?items))
    (str-cat ?*calls* " " (implode$ ?items)))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (traced (create$ 3 1 2)) crlf)
    (printout t (traced (create$ 4 2 3 1)) crlf)
    (printout t (traced (create$ 5 1 4 2 3)) crlf))
