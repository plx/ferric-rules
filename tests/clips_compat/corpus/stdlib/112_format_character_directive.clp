;; #340 pinned CLIPS characterization: character-directive

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%c|%4c|%-4c|%04c" 65 65 65 65) "]" crlf)
)
