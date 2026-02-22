; Basic ordered pattern matching
(deffacts startup
    (color red)
    (color blue)
    (color green))

(defrule report-color
    (color ?c)
    =>
    (printout t "Color: " ?c crlf))
