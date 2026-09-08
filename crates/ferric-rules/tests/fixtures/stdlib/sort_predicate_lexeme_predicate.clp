;; #343 pinned sort behavior: lexeme-predicate
(defglobal ?*result* = (create$))
(deffunction exchange (?a ?b) (> (str-compare ?a ?b) 0))
(deffacts startup (go))
(defrule exercise (go) =>
(bind ?*result* (sort exchange (create$ "z" a "b")))
(progn$ (?x ?*result*) (printout t (stringp ?x) ":" (symbolp ?x) ":" ?x crlf))
)
