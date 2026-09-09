(defglobal ?*trace* = 0)
(deffunction mark (?digit ?value) (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
(defrule exercise =>
(printout t "[" (mark 1 (create$ "a" "two words")) "|" (mark 2 "plain words") "]" crlf)
(printout t "trace:" ?*trace* crlf)
)
