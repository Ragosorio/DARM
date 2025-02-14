import { useState, useEffect } from "preact/hooks";

const NOTITAS = [
  "Feliz Día del Cariño, espero que te guste mucho este detallito. Cada día tendrás unas frases. 🌻",
  "Eres lo máximo, estoy muy orgulloso de ti, princesa. 💜",
  "Cada día más cerca de tu cumple, cada día más cerca de que te comas a un menor. 🫣",
  "Ve aceptando que te amo y que nuestra relación funcionará, con todo y sus defectos. 🙄",
  "¿Cómo te explico lo maravillosa que eres? 👀",
  "Espero que estés teniendo un lindo día... conmigo. :)",
  "No se trata de ser perfectos, se trata de ESTAR JUNTOS. 💜",
  "¿Ya te viste al espejo? Es que estás hermosa. 😘",
  "Uy, creo que ya casi es hora de que puedas tomar tequila legalmente. 🍻",
  "No sé ni qué decirte en este punto, estoy tan orgulloso de la persona en la que te has convertido. ¡Feliz casi cumpleaños! 💜"
]

function calcularTiempoRestante(fechaEvento) {
  const ahora = new Date();
  const diferencia = fechaEvento.getTime() - ahora.getTime();

  if (diferencia <= 0) {
    return {
      dias: 0,
      horas: 0,
      minutos: 0,
      segundos: 0,
      formato: "00d 00h 00m 00s",
    };
  }

  const dias = Math.floor(diferencia / (1000 * 60 * 60 * 24));
  const horas = Math.floor(
    (diferencia % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60)
  );
  const minutos = Math.floor((diferencia % (1000 * 60 * 60)) / (1000 * 60));
  const segundos = Math.floor((diferencia % (1000 * 60)) / 1000);

  const formato = `${dias}d ${horas}h ${minutos}m ${segundos}s`;

  return { dias, horas, minutos, segundos, formato };
}

export default function Contador() {
  const fechaEvento = new Date("2025-02-24T05:00:00Z");
  const [tiempoRestante, setTiempoRestante] = useState(
    calcularTiempoRestante(fechaEvento)
  );

  useEffect(() => {
    const intervalo = setInterval(() => {
      setTiempoRestante(calcularTiempoRestante(fechaEvento));
    }, 1000);

    return () => clearInterval(intervalo);
  }, []);

  const indiceNotita = 9 - Math.min(tiempoRestante.dias, 9);
  const notitaDia = NOTITAS[indiceNotita];  

  return (
    <div className="w-full h-dvh flex items-center justify-center flex-col text-2xl font-bold">
      <p class="absolute top-2 text-sm">Con 💜 para mi Mendi Mensita 💋</p>
      <p class="text-xl">aún no puedes verlo :) falta:</p>
      <p class="text-5xl pt-2">{`${tiempoRestante.dias} : ${tiempoRestante.horas} : ${tiempoRestante.minutos} : ${tiempoRestante.segundos}`}</p>
      <p class="absolute bottom-2 text-xs px-6 text-center text-pretty">{`Notita del día: ${notitaDia}`}</p>
    </div>
  );
}
