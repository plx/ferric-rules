(deffunction celsius-to-fahrenheit (?c)
    (+ (* ?c 1.8) 32))

(defrule report-temp
    (reading celsius ?c)
    =>
    (printout t ?c "C = " (celsius-to-fahrenheit ?c) "F" crlf))
